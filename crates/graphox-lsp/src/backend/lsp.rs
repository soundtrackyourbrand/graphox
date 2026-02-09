use super::{capabilities, fragment_manager, helpers};
use graphox_core::config::SchemaSource;
use graphox_core::document::DocumentLanguage;
use graphox_core::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap,
    FragmentDependentsMap, FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use graphox_core::{Config, DocumentState};

use graphox_features::call_hierarchy::DocumentCallHierarchy;
use graphox_features::code_actions::DocumentCodeActions;
use graphox_features::completion::{DocumentCompletion, FragmentCompletionInfo};
use graphox_features::definition::DocumentDefinition;
use graphox_features::document_highlight::DocumentHighlightFeature;
use graphox_features::folding_range::DocumentFoldingRange;
use graphox_features::hover::DocumentHover;
use graphox_features::references::DocumentReferences;
use graphox_features::selection_range::DocumentSelectionRange;
use graphox_features::semantic_tokens::DocumentSemanticTokens;
use graphox_features::shared::doc_utils;
use graphox_features::signature_help::DocumentSignatureHelp;
use graphox_features::symbols::DocumentSymbols;

use ahash::{AHashMap, AHashSet};
use apollo_compiler::Schema;
use dashmap::DashMap;
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result, lsp_types::*};

// Re-export ClientCapabilities for backward compatibility
pub use capabilities::ClientCapabilities;

pub struct Backend {
    pub client: Client,
    pub documents: DocumentsMap,
    pub config: Arc<std::sync::RwLock<Config>>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub empty_schema: Arc<Schema>,
    pub valid_empty_schema: Arc<apollo_compiler::validation::Valid<Schema>>,
    pub validated_schemas:
        Arc<DashMap<String, Arc<apollo_compiler::validation::Valid<Schema>>, ahash::RandomState>>,
    // Performance optimizations
    pub fragment_defs: FragmentDefsMap,
    pub fragment_spreads: FragmentSpreadsMap,
    pub package_roots: PackageRootsMap,
    pub fragment_dependents: FragmentDependentsMap,
    pub fragment_definitions: FragmentDefinitionsMap,
    /// Maps operation name -> (project schema key, URI)
    /// Used to detect duplicate operation names within a project
    pub operation_names: OperationNamesMap,
    pub workspace_loaded: Arc<AtomicBool>,
    pub open_documents: Arc<dashmap::DashSet<Url, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<AtomicBool>,
    pub gitignore: Arc<ignore::gitignore::Gitignore>,
    /// Persistent type cache per schema (keyed by schema key)
    /// Shared across all codegen runs for the same schema to maximize cache hits
    pub type_caches: Arc<DashMap<String, Arc<graphox_codegen::TypeCache>, ahash::RandomState>>,
    /// Client capabilities for conditional feature enablement
    pub client_capabilities: Arc<std::sync::RwLock<ClientCapabilities>>,
    /// Cached diagnostics for pull-based diagnostics (URI -> (version, diagnostics))
    pub diagnostic_cache: DiagnosticCacheMap,
    /// Throttled codegen runner
    pub codegen_throttle: Option<Arc<super::codegen_throttle::CodegenThrottle>>,
    /// Global cache for all fragments in the workspace
    pub fragment_metadata_cache: Arc<std::sync::RwLock<Option<Vec<FragmentCompletionInfo>>>>,
}

impl Backend {
    pub fn new(client: Client, mut config: Config) -> Self {
        // Canonicalize base_dir to ensure consistency on macOS
        if let Ok(canon) = std::fs::canonicalize(&config.base_dir) {
            config.base_dir = canon;
        }

        let schemas = DashMap::with_hasher(ahash::RandomState::default());
        let validated_schemas = DashMap::with_hasher(ahash::RandomState::default());
        let documents: DashMap<Url, Arc<DocumentState>, ahash::RandomState> =
            DashMap::with_hasher(ahash::RandomState::default());
        let fragment_definitions: DashMap<Arc<str>, AHashSet<Url>, ahash::RandomState> =
            DashMap::with_hasher(ahash::RandomState::default());

        let empty_schema = Arc::new(
            Schema::parse("type Query { _empty: String }", "empty.graphql").unwrap_or_else(|e| {
                super::error_logging::log_error_sync(format!(
                    "Failed to parse empty schema (this should never happen): {}",
                    e
                ));
                // Fallback to absolutely minimal schema
                Schema::parse(
                    "schema { query: Query } type Query { __typename: String }",
                    "fallback.graphql",
                )
                .expect("Critical error: even fallback schema failed to parse")
            }),
        );
        let valid_empty_schema = Arc::new((*empty_schema).clone().validate().unwrap_or_else(|e| {
            super::error_logging::log_error_sync(format!(
                "Failed to validate empty schema (this should never happen): {}",
                e
            ));
            // This really should never fail, but provide a panic with clear message
            panic!("Critical LSP initialization error: empty schema validation failed");
        }));

        // Load project schemas from config
        for project in &config.projects {
            let key = project.schema.as_key();
            if !schemas.contains_key(&key) {
                match Self::load_schema_source(&config.base_dir, &project.schema) {
                    Some(schema) => {
                        if let Ok(valid) = (*schema).clone().validate() {
                            validated_schemas.insert(key.clone(), Arc::new(valid));
                        } else {
                            super::error_logging::log_error_sync(format!(
                                "Schema validation failed for project '{}': schema is invalid",
                                key
                            ));
                        }
                        schemas.insert(key, schema);
                    }
                    None => {
                        super::error_logging::log_error_sync(format!(
                            "Failed to load schema for project '{}': schema files may be missing or invalid",
                            key
                        ));
                    }
                }
            }
        }

        let gitignore = Arc::new(graphox_core::utils::get_gitignore_matcher(&config.base_dir));

        let config_arc = Arc::new(std::sync::RwLock::new(config));
        let type_caches = Arc::new(DashMap::with_hasher(ahash::RandomState::default()));

        // Create codegen throttle if automatic codegen is enabled
        let codegen_throttle = {
            let cfg = config_arc.read().unwrap();
            if cfg.lsp_automatic_codegen() {
                Some(Arc::new(super::codegen_throttle::CodegenThrottle::new(
                    client.clone(),
                    config_arc.clone(),
                    type_caches.clone(),
                )))
            } else {
                None
            }
        };

        Self {
            client,
            documents: Arc::new(documents),
            config: config_arc,

            schemas: Arc::new(schemas),
            validated_schemas: Arc::new(validated_schemas),
            empty_schema,
            valid_empty_schema,
            fragment_defs: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            fragment_spreads: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            package_roots: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            fragment_dependents: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            fragment_definitions: Arc::new(fragment_definitions),
            operation_names: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            workspace_loaded: Arc::new(AtomicBool::new(false)),
            open_documents: Arc::new(dashmap::DashSet::with_hasher(ahash::RandomState::default())),
            workspace_scan_cancelled: Arc::new(AtomicBool::new(false)),
            gitignore,
            type_caches,
            client_capabilities: Arc::new(std::sync::RwLock::new(ClientCapabilities::default())),
            diagnostic_cache: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            codegen_throttle,
            fragment_metadata_cache: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    fn load_schema_source(
        base_dir: &std::path::Path,
        source: &SchemaSource,
    ) -> Option<Arc<Schema>> {
        graphox_core::schema::load_schema_arc(base_dir, source)
    }

    pub fn normalize_uri(&self, uri: Url) -> Url {
        helpers::normalize_uri(uri)
    }

    pub fn get_schema_for_doc(&self, uri: &Url) -> Arc<apollo_compiler::validation::Valid<Schema>> {
        let config = self.config.read().unwrap();
        super::validation::get_schema_for_doc(
            uri,
            &config,
            &self.validated_schemas,
            &self.valid_empty_schema,
        )
    }

    pub fn get_all_fragments_info(&self) -> Vec<FragmentCompletionInfo> {
        // Try to return cached metadata if available
        if let Ok(cache) = self.fragment_metadata_cache.read()
            && let Some(metadata) = &*cache
        {
            return metadata.clone();
        }

        // Cache miss: collect metadata and update cache
        let config = self.config.read().unwrap();
        let metadata = fragment_manager::collect_fragment_metadata(
            &self.fragment_defs,
            &config,
            &self.package_roots,
        );

        if let Ok(mut cache) = self.fragment_metadata_cache.write() {
            *cache = Some(metadata.clone());
        }

        metadata
    }

    pub fn invalidate_fragment_cache(&self) {
        if let Ok(mut cache) = self.fragment_metadata_cache.write() {
            *cache = None;
        }
    }

    pub fn get_fragments_for_doc(
        &self,
        doc: &DocumentState,
        all_fragments: &[FragmentCompletionInfo],
    ) -> Vec<FragmentCompletionInfo> {
        let config = self.config.read().unwrap();
        super::validation::get_fragments_for_doc_with_metadata(doc, &config, all_fragments)
    }

    fn get_transitive_fragments(
        &self,
        initial_spreads: Vec<Arc<str>>,
        package_root: Option<&std::path::PathBuf>,
    ) -> AHashSet<Url> {
        let mut visited_names = AHashSet::default();
        let mut fragment_uris = AHashSet::default();
        let mut to_visit = initial_spreads;

        let all_fragments = self.get_all_fragments_info();

        while let Some(name) = to_visit.pop() {
            if !visited_names.insert(name.clone()) {
                continue;
            }

            // Find this fragment (respecting scoping)
            if let Some(frag) = all_fragments.iter().find(|f| {
                f.name == name
                    && (f.is_public
                        || graphox_core::utils::paths_match(
                            f.package_root.as_deref(),
                            package_root.map(|p| p.as_path()),
                        ))
            }) {
                fragment_uris.insert(frag.uri.clone());

                // Add its nested spreads
                if let Some(doc) = self.documents.get(&frag.uri).map(|r| r.value().clone()) {
                    // Find the specific fragment def in the doc to get its spreads
                    if let Some(def) = doc.fragments().iter().find(|f| f.name == name) {
                        for nested in &def.used_fragments {
                            to_visit.push(nested.clone());
                        }
                    }
                }
            }
        }

        fragment_uris
    }

    pub fn get_fragment_requirements(
        &self,
        name: &str,
        schema: &Schema,
        package_root: Option<&std::path::PathBuf>,
        all_fragments: &[FragmentCompletionInfo],
        variable_types_cache: &mut AHashMap<
            Arc<str>,
            std::collections::BTreeMap<Arc<str>, Arc<str>>,
        >,
    ) -> std::collections::BTreeMap<Arc<str>, Arc<str>> {
        let mut requirements = std::collections::BTreeMap::new();
        let mut visited = AHashSet::default();

        let mut collect = |initial_name: &str| {
            let mut stack: Vec<Arc<str>> = vec![Arc::from(initial_name)];

            while let Some(name) = stack.pop() {
                if !visited.insert(name.clone()) {
                    continue;
                }

                if let Some(frag) = all_fragments.iter().find(|f| {
                    f.name == name
                        && (f.is_public
                            || graphox_core::utils::paths_match(
                                f.package_root.as_deref(),
                                package_root.map(|p| p.as_path()),
                            ))
                }) && let Some(doc) = self.documents.get(&frag.uri).map(|r| r.value().clone())
                {
                    let local_vars = if let Some(cached) = variable_types_cache.get(&name) {
                        cached.clone()
                    } else {
                        let vars = doc.get_fragment_variable_types(&name, schema);
                        let mut vars_arc = std::collections::BTreeMap::new();
                        for (k, v) in vars {
                            vars_arc.insert(Arc::from(k), Arc::from(v));
                        }
                        variable_types_cache.insert(name.clone(), vars_arc.clone());
                        vars_arc
                    };

                    for (var, ty) in local_vars {
                        requirements.insert(var, ty);
                    }

                    if let Some(def) = doc.fragments().iter().find(|f| f.name == name) {
                        for nested in &def.used_fragments {
                            stack.push(nested.clone());
                        }
                    }
                }
            }
        };

        collect(name);
        requirements
    }

    pub fn get_used_fragments(&self) -> AHashSet<Arc<str>> {
        super::validation::get_used_fragments(&self.fragment_spreads)
    }

    async fn with_tracing<T, Fut>(&self, name: &str, fut: Fut) -> Result<Option<T>>
    where
        Fut: std::future::Future<Output = Result<Option<T>>>,
    {
        let timeout_ms = {
            let config = self.config.read().unwrap();
            config.get_timeouts().lsp_request_ms
        };

        let tracing_config = {
            let config = self.config.read().unwrap();
            config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
        };

        helpers::with_tracing(&self.client, name, timeout_ms, tracing_config, fut).await
    }

    async fn reload_schema(&self, changed_path: &str) {
        let supports_progress = self
            .client_capabilities
            .read()
            .map(|caps| caps.supports_progress)
            .unwrap_or(false);

        let config = self.config.read().unwrap().clone();
        let reloaded_keys = super::schema_management::reload_schema(
            changed_path,
            &config,
            &self.schemas,
            &self.validated_schemas,
            &self.client,
            supports_progress,
        )
        .await;

        if !reloaded_keys.is_empty() {
            // Invalidate fragment metadata cache since schema changed,
            // which might affect fragment requirements resolution.
            self.invalidate_fragment_cache();
        }

        // Validate documents affected by reloaded schemas
        for key in reloaded_keys {
            let affected =
                super::schema_management::get_uris_affected_by_schema(&key, &config, || {
                    self.documents.iter().map(|e| e.key().clone()).collect()
                });
            self.validate_uris(affected).await;
        }
    }

    pub async fn clear_cache(&self) {
        let config = self.config.read().unwrap().clone();

        // Clear fragment metadata cache
        self.invalidate_fragment_cache();

        // Clear globset cache in config
        graphox_core::config::clear_globset_cache();

        // Clear schema memory and disk cache
        let _ = graphox_core::schema_cache::clear_cache();

        // Clear all internal state
        self.schemas.clear();
        self.validated_schemas.clear();

        // Clear all documents (including open ones) to ensure full re-load
        self.documents.clear();

        self.fragment_defs.clear();
        self.fragment_spreads.clear();
        self.fragment_dependents.clear();
        self.fragment_definitions.clear();
        self.package_roots.clear();
        self.type_caches.clear();
        self.diagnostic_cache.clear();

        // Trigger workspace scan to re-index everything
        let supports_progress = self
            .client_capabilities
            .read()
            .map(|caps| caps.supports_progress)
            .unwrap_or(false);

        // Reset workspace_loaded flag
        self.workspace_loaded
            .store(false, std::sync::atomic::Ordering::Relaxed);

        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config: config.clone(),
            documents: self.documents.clone(),
            fragment_defs: self.fragment_defs.clone(),
            fragment_spreads: self.fragment_spreads.clone(),
            package_roots: self.package_roots.clone(),
            fragment_dependents: self.fragment_dependents.clone(),
            fragment_definitions: self.fragment_definitions.clone(),
            operation_names: self.operation_names.clone(),
            workspace_loaded: self.workspace_loaded.clone(),
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            supports_progress,
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
        });

        self.client
            .log_message(
                MessageType::INFO,
                "All caches cleared and workspace re-scanned!",
            )
            .await;
    }

    pub async fn run_codegen(&self) {
        let supports_progress = self
            .client_capabilities
            .read()
            .map(|caps| caps.supports_progress)
            .unwrap_or(false);

        let config = self.config.read().unwrap().clone();
        super::codegen_runner::run_codegen(
            self.client.clone(),
            config,
            self.type_caches.clone(),
            supports_progress,
        )
        .await;
    }

    /// Reloads the configuration file and reinitializes the LSP state
    async fn reload_config(&self) {
        self.client
            .log_message(
                MessageType::INFO,
                "Configuration file changed, reloading...",
            )
            .await;

        // Clear fragment metadata cache
        self.invalidate_fragment_cache();

        // Clear globset cache in config
        graphox_core::config::clear_globset_cache();

        // Get the base directory from current config
        let base_dir = self.config.read().unwrap().base_dir.clone();

        // Try to load new config
        let new_config = match Config::load_from_dir(&base_dir) {
            Some(config) => config,
            None => {
                self.client
                    .log_message(MessageType::ERROR, "Failed to reload configuration file")
                    .await;
                return;
            }
        };

        // Update the config
        *self.config.write().unwrap() = new_config;

        // Clear all state
        self.schemas.clear();
        self.validated_schemas.clear();
        graphox_core::schema_cache::clear_memory_cache();

        // Clear all documents
        self.documents.clear();

        self.fragment_defs.clear();
        self.fragment_spreads.clear();
        self.fragment_dependents.clear();
        self.fragment_definitions.clear();
        self.package_roots.clear();
        self.type_caches.clear();
        self.diagnostic_cache.clear();

        // Re-register file watchers with new config
        {
            let config = self.config.read().unwrap();
            super::file_watchers::register_file_watchers(self.client.clone(), &config);
        }

        // Trigger workspace scan to re-index everything
        let supports_progress = self
            .client_capabilities
            .read()
            .map(|caps| caps.supports_progress)
            .unwrap_or(false);

        // Reset workspace_loaded flag
        self.workspace_loaded
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let scan_config = self.config.read().unwrap().clone();
        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config: scan_config,
            documents: self.documents.clone(),
            fragment_defs: self.fragment_defs.clone(),
            fragment_spreads: self.fragment_spreads.clone(),
            package_roots: self.package_roots.clone(),
            fragment_dependents: self.fragment_dependents.clone(),
            fragment_definitions: self.fragment_definitions.clone(),
            operation_names: self.operation_names.clone(),
            workspace_loaded: self.workspace_loaded.clone(),
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            supports_progress,
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
        });

        self.client
            .log_message(MessageType::INFO, "Configuration reloaded successfully")
            .await;
    }
    fn update_dependency_indices(
        &self,
        uri: &Url,
        old_spreads: Option<Vec<Arc<str>>>,
        new_spreads: Vec<Arc<str>>,
    ) {
        fragment_manager::update_fragment_dependents(
            &self.fragment_dependents,
            uri,
            old_spreads,
            new_spreads,
        );
    }

    fn update_definition_indices(
        &self,
        uri: &Url,
        old_fragments: Option<Vec<Arc<str>>>,
        new_fragments: Vec<Arc<str>>,
    ) {
        fragment_manager::update_fragment_definitions(
            &self.fragment_definitions,
            uri,
            old_fragments,
            new_fragments,
        );
    }

    pub async fn validate_uris(&self, uris: Vec<Url>) {
        let (use_push, supports_progress) = if let Ok(caps) = self.client_capabilities.read() {
            (!caps.supports_pull_diagnostics, caps.supports_progress)
        } else {
            (true, false) // Default to push if can't read capabilities
        };

        let config = self.config.read().unwrap().clone();
        let params = super::validation::ValidationParams {
            client: &self.client,
            documents: &self.documents,
            config: &config,
            fragment_defs: &self.fragment_defs,
            fragment_spreads: &self.fragment_spreads,
            package_roots: &self.package_roots,
            validated_schemas: &self.validated_schemas,
            valid_empty_schema: &self.valid_empty_schema,
            workspace_loaded: &self.workspace_loaded,
            open_documents: &self.open_documents,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
            operation_names: &self.operation_names,
            supports_progress,
        };
        super::validation::validate_uris(params, uris, use_push, Some(&self.diagnostic_cache))
            .await;
    }

    pub async fn validate_all_documents(&self) {
        let (use_push, supports_progress) = if let Ok(caps) = self.client_capabilities.read() {
            (!caps.supports_pull_diagnostics, caps.supports_progress)
        } else {
            (true, false) // Default to push if can't read capabilities
        };

        let config = self.config.read().unwrap().clone();
        let params = super::validation::ValidationParams {
            client: &self.client,
            documents: &self.documents,
            config: &config,
            fragment_defs: &self.fragment_defs,
            fragment_spreads: &self.fragment_spreads,
            package_roots: &self.package_roots,
            validated_schemas: &self.validated_schemas,
            valid_empty_schema: &self.valid_empty_schema,
            workspace_loaded: &self.workspace_loaded,
            open_documents: &self.open_documents,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
            operation_names: &self.operation_names,
            supports_progress,
        };
        super::validation::validate_all_documents(params, use_push, Some(&self.diagnostic_cache))
            .await;
    }

    fn get_affected_uris(
        &self,
        initial_uri: Url,
        affected_fragment_names: AHashSet<Arc<str>>,
        affected_spread_names: AHashSet<Arc<str>>,
    ) -> Vec<Url> {
        super::validation::get_affected_uris(
            initial_uri,
            affected_fragment_names,
            affected_spread_names,
            &self.documents,
            &self.fragment_dependents,
            &self.fragment_definitions,
        )
    }

    /// Try to find variable definition location
    fn get_preferred_schema_uris(&self, uri: &Url) -> Vec<Url> {
        let mut preferred_uris = Vec::new();
        if let Ok(path) = uri.to_file_path() {
            let config = self.config.read().unwrap();
            if let Some(project) = config.get_project_for_path(&path) {
                for schema_file in project.schema.files() {
                    let schema_path = config.base_dir.join(schema_file);
                    if let Ok(schema_uri) = Url::from_file_path(schema_path) {
                        preferred_uris.push(schema_uri);
                    }
                }
            }
        }
        preferred_uris
    }

    /// Try to find fragment definition location
    async fn try_goto_fragment_definition(
        &self,
        symbol_name: &Option<String>,
        doc: &Arc<DocumentState>,
    ) -> Option<Location> {
        let name = symbol_name.as_ref()?;

        // Try targeted lookup using the index first
        if let Some(location) = self.lookup_fragment_in_index(name, doc) {
            return Some(location);
        }

        // Fallback to full scan if not in index
        self.scan_all_documents_for_fragment(name, doc).await
    }

    /// Look up fragment definition using the fragment index
    fn lookup_fragment_in_index(&self, name: &str, doc: &Arc<DocumentState>) -> Option<Location> {
        let uris = self.fragment_definitions.get(name)?;

        for other_uri in uris.iter() {
            let other_doc = self.documents.get(other_uri).map(|r| r.value().clone())?;

            if self.is_fragment_accessible(&other_doc, doc, name)
                && let Some(location) = other_doc.find_definition_in_tree(name)
            {
                return Some(location);
            }
        }

        None
    }

    /// Scan all documents for fragment definition (fallback)
    async fn scan_all_documents_for_fragment(
        &self,
        name: &str,
        doc: &Arc<DocumentState>,
    ) -> Option<Location> {
        // Only scan if not in index
        if self.fragment_definitions.contains_key(name) {
            return None;
        }

        let doc_arcs: Vec<Arc<DocumentState>> =
            self.documents.iter().map(|e| e.value().clone()).collect();

        doc_arcs.par_iter().find_map_any(|other_doc| {
            if self.is_fragment_accessible(other_doc, doc, name) {
                other_doc.find_definition_in_tree(name)
            } else {
                None
            }
        })
    }

    /// Check if a fragment is accessible from the current document
    fn is_fragment_accessible(
        &self,
        fragment_doc: &Arc<DocumentState>,
        current_doc: &Arc<DocumentState>,
        fragment_name: &str,
    ) -> bool {
        let is_same_package = graphox_core::utils::paths_match(
            fragment_doc.package_root.as_deref(),
            current_doc.package_root.as_deref(),
        );
        let is_public_fragment = fragment_doc
            .fragments()
            .iter()
            .any(|f| f.name.as_ref() == fragment_name && f.is_public);

        is_same_package || is_public_fragment
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Extract and store client capabilities
        let caps = ClientCapabilities::from_params(&params);

        // Store capabilities
        if let Ok(mut stored_caps) = self.client_capabilities.write() {
            *stored_caps = caps.clone();
        }

        // Log detected capabilities
        self.client
            .log_message(
                MessageType::INFO,
                format!("Client capabilities: {}", caps.to_log_string()),
            )
            .await;

        Ok(InitializeResult {
            capabilities: capabilities::build_server_capabilities(&caps),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "LSP Started!")
            .await;

        // Check tracing configuration and log if enabled
        let tracing_msg = {
            let config = self.config.read().unwrap();
            config.tracing.as_ref().and_then(|tracing| {
                if tracing.enabled {
                    Some(format!(
                        "Performance tracing enabled (threshold: {}ms)",
                        tracing.threshold_ms
                    ))
                } else {
                    None
                }
            })
        };

        if let Some(msg) = tracing_msg {
            self.client.log_message(MessageType::INFO, msg).await;
        }

        // Spawn workspace scan in background to avoid hanging the LSP
        let supports_progress = self
            .client_capabilities
            .read()
            .map(|caps| caps.supports_progress)
            .unwrap_or(false);

        let config = self.config.read().unwrap().clone();
        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config,
            documents: self.documents.clone(),
            fragment_defs: self.fragment_defs.clone(),
            fragment_spreads: self.fragment_spreads.clone(),
            package_roots: self.package_roots.clone(),
            fragment_dependents: self.fragment_dependents.clone(),
            fragment_definitions: self.fragment_definitions.clone(),
            operation_names: self.operation_names.clone(),
            workspace_loaded: self.workspace_loaded.clone(),
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            supports_progress,
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
        });

        // Register file watchers
        let config = self.config.read().unwrap();
        super::file_watchers::register_file_watchers(self.client.clone(), &config);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        self.with_tracing("hover", async move {
            let uri = self.normalize_uri(params.text_document_position_params.text_document.uri);
            let position = params.text_document_position_params.position;

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = self.get_schema_for_doc(&uri);
                if let Some(hover) = doc.get_hover_info(position, &schema) {
                    return Ok(Some(hover));
                }

                if let Some(symbol_name) = doc.get_symbol_at_position(position) {
                    // Collect documents first to avoid holding DashMap locks during processing
                    let doc_arcs: Vec<Arc<DocumentState>> =
                        self.documents.iter().map(|e| e.value().clone()).collect();

                    for other_doc in doc_arcs {
                        let is_same_package = graphox_core::utils::paths_match(
                            other_doc.package_root.as_deref(),
                            doc.package_root.as_deref(),
                        );
                        let is_public_fragment = other_doc
                            .fragments()
                            .iter()
                            .any(|f| f.name.as_ref() == symbol_name && f.is_public);

                        if (is_same_package || is_public_fragment)
                            && let Some(info) = other_doc.find_fragment_info(&symbol_name)
                        {
                            let mut value = format!("```graphql\n{}\n```", info);

                            let all_fragments = self.get_all_fragments_info();
                            let mut variable_types_cache = AHashMap::default();
                            let requirements = self.get_fragment_requirements(
                                &symbol_name,
                                &schema,
                                doc.package_root.as_ref(),
                                &all_fragments,
                                &mut variable_types_cache,
                            );
                            if !requirements.is_empty() {
                                value.push_str("\n\n**Requires Variables:**\n");
                                for (var, ty) in requirements {
                                    value.push_str(&format!("- `${}`: `{}`\n", var, ty));
                                }
                            }

                            if let Some(desc) =
                                doc_utils::find_description(&other_doc, &symbol_name)
                            {
                                value.push_str("\n\n---\n");
                                value.push_str(&desc);
                            }

                            if !is_same_package && let Ok(other_p) = other_doc.uri.to_file_path() {
                                let config = self.config.read().unwrap();
                                if let Some(proj) = config.get_project_for_path(&other_p)
                                    && let Some(import) = &proj.import
                                {
                                    value.push_str("\n\n---\n");
                                    value.push_str(&format!("Import: `{}`", import));
                                }
                            }

                            return Ok(Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value,
                                }),
                                range: None,
                            }));
                        }
                    }
                }
            }

            Ok(None)
        })
        .await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.with_tracing("completion", async move {
            let uri = self.normalize_uri(params.text_document_position.text_document.uri);
            let position = params.text_document_position.position;

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = self.get_schema_for_doc(&uri);
                let all_fragments = self.get_all_fragments_info();

                // Optimization: Identify completion context first.
                // If we are not in a selection set, we can skip fragments entirely.
                let context = doc.get_completion_context(position, &schema);

                let mut fragments = match context {
                    graphox_core::document::CompletionContext::SelectionSet(parent_type) => {
                        let mut filtered = self.get_fragments_for_doc(&doc, &all_fragments);
                        let parent_name = parent_type.name();

                        filtered.retain(|f| {
                            if f.is_type_only {
                                return false;
                            }
                            // Keep fragment if it's on the same type
                            if f.type_condition.as_ref() == parent_name.as_str() {
                                return true;
                            }

                            // Get the fragment's type from schema
                            let frag_type = match schema.types.get(f.type_condition.as_ref()) {
                                Some(t) => t,
                                None => return true, // If type unknown, play it safe and keep it
                            };

                            // Check for intersection between parent_type and frag_type
                            match (&parent_type, frag_type) {
                                // Object and Interface/Object
                                (
                                    apollo_compiler::schema::ExtendedType::Object(obj),
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                ) => obj
                                    .implements_interfaces
                                    .iter()
                                    .any(|i| i.as_str() == f.type_condition.as_ref()),
                                (
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                    apollo_compiler::schema::ExtendedType::Object(obj),
                                ) => obj
                                    .implements_interfaces
                                    .iter()
                                    .any(|i| i.as_str() == parent_name.as_str()),

                                // Union cases
                                (
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                    apollo_compiler::schema::ExtendedType::Object(_),
                                ) => u
                                    .members
                                    .iter()
                                    .any(|m| m.as_str() == f.type_condition.as_ref()),
                                (
                                    apollo_compiler::schema::ExtendedType::Object(_),
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                ) => u.members.iter().any(|m| m.as_str() == parent_name.as_str()),

                                // Interface and Interface (intersection if they share implementors)
                                (
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                ) => true,

                                // Union and Interface
                                (
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                ) => u.members.iter().any(|m| {
                                    if let Some(apollo_compiler::schema::ExtendedType::Object(
                                        obj,
                                    )) = schema.types.get(m.as_str())
                                    {
                                        obj.implements_interfaces
                                            .iter()
                                            .any(|i| i.as_str() == f.type_condition.as_ref())
                                    } else {
                                        false
                                    }
                                }),
                                (
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                ) => u.members.iter().any(|m| {
                                    if let Some(apollo_compiler::schema::ExtendedType::Object(
                                        obj,
                                    )) = schema.types.get(m.as_str())
                                    {
                                        obj.implements_interfaces
                                            .iter()
                                            .any(|i| i.as_str() == parent_name.as_str())
                                    } else {
                                        false
                                    }
                                }),

                                // Union and Union
                                (
                                    apollo_compiler::schema::ExtendedType::Union(u1),
                                    apollo_compiler::schema::ExtendedType::Union(u2),
                                ) => u1.members.iter().any(|m1| {
                                    u2.members.iter().any(|m2| m1.as_str() == m2.as_str())
                                }),

                                _ => false,
                            }
                        });
                        filtered
                    }
                    graphox_core::document::CompletionContext::OperationDefinition => Vec::new(),
                    graphox_core::document::CompletionContext::SchemaDefinition => Vec::new(),
                    graphox_core::document::CompletionContext::FieldAlias => Vec::new(),
                    graphox_core::document::CompletionContext::DirectiveArguments => Vec::new(),
                    graphox_core::document::CompletionContext::UnionMembers => Vec::new(),
                    graphox_core::document::CompletionContext::ImplementsClause => Vec::new(),
                    graphox_core::document::CompletionContext::VariableDefaultValue => Vec::new(),
                    graphox_core::document::CompletionContext::ArgumentDefaultValue => Vec::new(),
                    graphox_core::document::CompletionContext::Other => Vec::new(),
                };

                log::trace!(
                    "completion: fragments for doc {} = {:?}",
                    doc.uri,
                    fragments.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
                );

                let mut variable_types_cache = AHashMap::default();
                for f in &mut fragments {
                    f.requirements = self.get_fragment_requirements(
                        &f.name,
                        &schema,
                        doc.package_root.as_ref(),
                        &all_fragments,
                        &mut variable_types_cache,
                    );
                }

                let items = doc.get_completion_items(position, &schema, fragments);
                log::trace!(
                    "completion: produced items = {:?}",
                    items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
                );
                return Ok(Some(CompletionResponse::Array(items)));
            }

            Ok(None)
        })
        .await
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        self.with_tracing("signature_help", async move {
            let uri = self.normalize_uri(params.text_document_position_params.text_document.uri);
            let position = params.text_document_position_params.position;

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = self.get_schema_for_doc(&uri);
                return Ok(doc.get_signature_help(position, &schema));
            }

            Ok(None)
        })
        .await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        self.with_tracing("prepare_call_hierarchy", async move {
            let uri = self.normalize_uri(
                params
                    .text_document_position_params
                    .text_document
                    .uri
                    .clone(),
            );
            let position = params.text_document_position_params.position;

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                return Ok(doc.prepare_call_hierarchy(position));
            }

            Ok(None)
        })
        .await
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        self.with_tracing("incoming_calls", async move {
            let item = params.item;
            let symbol_name = item.name;
            let mut incoming = Vec::new();

            // Collect documents first to avoid holding DashMap locks during processing
            let doc_arcs: Vec<Arc<DocumentState>> =
                self.documents.iter().map(|e| e.value().clone()).collect();

            for doc in doc_arcs {
                let refs = doc.find_references_in_tree(&symbol_name, false);

                if !refs.is_empty() {
                    // For each reference, we need to find the container (fragment or operation)
                    // This is a bit expensive but necessary for call hierarchy.
                    // For now, let's group by URI.

                    let mut ranges_by_container: std::collections::HashMap<String, Vec<Range>> =
                        std::collections::HashMap::new();

                    // Grouping is hard because we need the container name.
                    // Let's simplify: each reference is its own call from the file.
                    for r in refs {
                        // Try to find what container this range is in
                        let container_name = doc.get_container_name_at_range(r.range);
                        let key = container_name.unwrap_or_else(|| "unknown".to_string());
                        ranges_by_container.entry(key).or_default().push(r.range);
                    }

                    for (name, ranges) in ranges_by_container {
                        incoming.push(CallHierarchyIncomingCall {
                            from: CallHierarchyItem {
                                name: name.clone(),
                                kind: SymbolKind::FUNCTION,
                                tags: None,
                                detail: Some(doc.uri.to_string()),
                                uri: doc.uri.clone(),
                                range: doc
                                    .find_definition_in_tree(&name)
                                    .map(|l| l.range)
                                    .unwrap_or(ranges[0]),
                                selection_range: doc
                                    .find_definition_in_tree(&name)
                                    .map(|l| l.range)
                                    .unwrap_or(ranges[0]),
                                data: None,
                            },
                            from_ranges: ranges,
                        });
                    }
                }
            }

            if incoming.is_empty() {
                Ok(None)
            } else {
                Ok(Some(incoming))
            }
        })
        .await
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        self.with_tracing("outgoing_calls", async move {
            let item = params.item;
            let symbol_name = item.name;
            let uri = self.normalize_uri(item.uri);

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let mut calls = doc.get_outgoing_calls(&symbol_name);

                // Collect documents first to avoid holding DashMap locks during processing
                let doc_arcs: Vec<Arc<DocumentState>> =
                    self.documents.iter().map(|e| e.value().clone()).collect();

                // Resolve the 'to' items
                for call in &mut calls {
                    let callee_name = &call.to.name;
                    // Find where it's defined
                    for other_doc in &doc_arcs {
                        if let Some(loc) = other_doc.find_definition_in_tree(callee_name) {
                            call.to.uri = loc.uri;
                            call.to.range = loc.range;
                            call.to.selection_range = loc.range;
                            break;
                        }
                    }
                }

                return Ok(Some(calls));
            }

            Ok(None)
        })
        .await
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri.clone());
        self.open_documents.insert(uri.clone());
        let language = DocumentLanguage::from_uri(&uri);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.get_parser_language())
            .unwrap();

        let doc = DocumentState::new(uri.clone(), &params.text_document.text, parser);

        let mut affected_fragment_names = AHashSet::default();
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
        }

        // Update performance indices
        self.invalidate_fragment_cache();
        self.fragment_defs
            .insert(uri.clone(), doc.fragments().to_vec());
        self.fragment_spreads
            .insert(uri.clone(), doc.fragment_spreads.clone());
        self.package_roots
            .insert(uri.clone(), doc.package_root.clone());
        self.update_dependency_indices(&uri, None, doc.fragment_spreads.clone());
        self.update_definition_indices(
            &uri,
            None,
            doc.fragments().iter().map(|f| f.name.clone()).collect(),
        );

        let mut affected_spread_names = AHashSet::default();
        for s in &doc.fragment_spreads {
            affected_spread_names.insert(s.clone());
        }

        self.documents.insert(uri.clone(), Arc::new(doc));

        let uris_to_validate =
            self.get_affected_uris(uri, affected_fragment_names, affected_spread_names);
        self.validate_uris(uris_to_validate).await;

        // Request throttled codegen if enabled
        if let Some(throttle) = &self.codegen_throttle {
            throttle.request_codegen();
        }
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri);
        self.open_documents.remove(&uri);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri.clone());
        let version = params.text_document.version;

        // Process document changes and update indices
        let change_params = super::document_changes::DocumentChangeParams {
            documents: &self.documents,
            fragment_defs: &self.fragment_defs,
            fragment_spreads: &self.fragment_spreads,
            package_roots: &self.package_roots,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
        };

        if let Some(result) = super::document_changes::process_document_change(
            &uri,
            params.content_changes,
            version,
            &change_params,
        ) {
            // Invalidate fragment metadata cache since fragments might have changed
            self.invalidate_fragment_cache();

            // Validate affected documents
            self.validate_uris(result.uris_to_validate).await;

            // Request throttled codegen if enabled
            if let Some(throttle) = &self.codegen_throttle {
                throttle.request_codegen();
            }
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        self.with_tracing("goto_definition", async move {
            let uri = self.normalize_uri(params.text_document_position_params.text_document.uri);
            let position = params.text_document_position_params.position;

            let doc_arc = if let Some(d) = self.documents.get(&uri).map(|r| r.value().clone()) {
                d
            } else {
                return Ok(None);
            };

            let symbol_name = doc_arc.get_symbol_at_position(position);
            let schema = self.get_schema_for_doc(&uri);

            // 1. Try unified definition lookup using the shared resolver
            let preferred_uris = self.get_preferred_schema_uris(&uri);
            if let Some(location) =
                doc_arc.get_definition(position, &schema, &self.documents, &preferred_uris)
            {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            // 2. Fallback to fragment definition (requires fragment index in Backend)
            if let Some(location) = self
                .try_goto_fragment_definition(&symbol_name, &doc_arc)
                .await
            {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            Ok(None)
        })
        .await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        self.with_tracing("references", async move {
            let uri = self.normalize_uri(params.text_document_position.text_document.uri.clone());
            let position = params.text_document_position.position;
            let include_declaration = params.context.include_declaration;

            let symbol_name = if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone())
            {
                doc.get_symbol_at_position(position)
            } else {
                None
            };

            if let Some(name) = symbol_name {
                if name.starts_with('$') {
                    if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                        let mut all_refs =
                            doc.find_variable_references(&name, position, include_declaration);

                        // Find transitive references in fragments
                        if let Some((op_node, offset)) =
                            doc.find_containing_operation_node(position)
                        {
                            let initial_spreads = doc.get_fragment_spreads_in_node(op_node, offset);
                            let frag_uris = self.get_transitive_fragments(
                                initial_spreads,
                                doc.package_root.as_ref(),
                            );

                            for f_uri in frag_uris {
                                if let Some(f_doc) =
                                    self.documents.get(&f_uri).map(|r| r.value().clone())
                                {
                                    let frag_refs = f_doc.find_references_in_tree(&name, false);
                                    all_refs.extend(frag_refs);
                                }
                            }
                        }

                        return Ok(if all_refs.is_empty() {
                            None
                        } else {
                            Some(all_refs)
                        });
                    }
                    return Ok(None);
                }

                let mut all_references = Vec::new();

                let doc_arcs: Vec<Arc<DocumentState>> =
                    self.documents.iter().map(|e| e.value().clone()).collect();

                for other_doc in doc_arcs {
                    let refs = other_doc.find_references_in_tree(&name, include_declaration);
                    all_references.extend(refs);
                }

                if all_references.is_empty() {
                    return Ok(None);
                }

                return Ok(Some(all_references));
            }

            Ok(None)
        })
        .await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        self.with_tracing("document_highlight", async move {
            let uri = self.normalize_uri(
                params
                    .text_document_position_params
                    .text_document
                    .uri
                    .clone(),
            );
            let position = params.text_document_position_params.position;

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = self.get_schema_for_doc(&uri);
                return Ok(doc.get_document_highlights(position, &schema));
            }

            Ok(None)
        })
        .await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        self.with_tracing("rename", async move {
            let uri = self.normalize_uri(params.text_document_position.text_document.uri.clone());
            let position = params.text_document_position.position;
            let new_name = params.new_name;

            let symbol_name = if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone())
            {
                doc.get_symbol_at_position(position)
            } else {
                None
            };

            if let Some(name) = symbol_name {
                let mut changes = std::collections::HashMap::new();

                if name.starts_with('$') {
                    if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                        let refs = doc.find_variable_references(&name, position, true);
                        if !refs.is_empty() {
                            let edits: Vec<TextEdit> = refs
                                .into_iter()
                                .map(|loc| TextEdit {
                                    range: loc.range,
                                    new_text: new_name.clone(),
                                })
                                .collect();
                            changes.insert(uri.clone(), edits);
                        }
                    }
                    return Ok(if changes.is_empty() {
                        None
                    } else {
                        Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        })
                    });
                }

                let doc_arcs: Vec<(Url, Arc<DocumentState>)> = self
                    .documents
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();

                for (other_uri, other_doc) in doc_arcs {
                    let refs = other_doc.find_references_in_tree(&name, true);
                    if !refs.is_empty() {
                        let edits: Vec<TextEdit> = refs
                            .into_iter()
                            .map(|loc| TextEdit {
                                range: loc.range,
                                new_text: new_name.clone(),
                            })
                            .collect();
                        changes.insert(other_uri.clone(), edits);
                    }
                }

                return Ok(Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }));
            }

            Ok(None)
        })
        .await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        self.with_tracing("prepare_rename", async move {
            let uri = self.normalize_uri(params.text_document.uri.clone());
            let position = params.position;

            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone())
                && let Some(_name) = doc.get_symbol_at_position(position)
            {
                return Ok(Some(PrepareRenameResponse::DefaultBehavior {
                    default_behavior: true,
                }));
            }

            Ok(None)
        })
        .await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        self.with_tracing("document_symbol", async move {
            let uri = self.normalize_uri(params.text_document.uri);
            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let symbols = doc.get_symbols();
                return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
            }
            Ok(None)
        })
        .await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        self.with_tracing("semantic_tokens_full", async move {
            let uri = self.normalize_uri(params.text_document.uri);
            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let tokens = doc.get_semantic_tokens();
                return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                    result_id: None,
                    data: tokens,
                })));
            }
            Ok(None)
        })
        .await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        self.with_tracing("folding_range", async move {
            let uri = self.normalize_uri(params.text_document.uri);
            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let ranges = doc.get_folding_ranges();
                return Ok(if ranges.is_empty() {
                    None
                } else {
                    Some(ranges)
                });
            }
            Ok(None)
        })
        .await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        self.with_tracing("selection_range", async move {
            let uri = self.normalize_uri(params.text_document.uri);
            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let ranges = doc.get_selection_ranges(params.positions);
                return Ok(if ranges.is_empty() {
                    None
                } else {
                    Some(ranges)
                });
            }
            Ok(None)
        })
        .await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.with_tracing("workspace_symbol", async move {
            let query = params.query.to_lowercase();
            let mut all_symbols = Vec::new();

            let doc_arcs: Vec<Arc<DocumentState>> =
                self.documents.iter().map(|e| e.value().clone()).collect();

            for doc in doc_arcs {
                let symbols = doc.get_symbols();

                for sym in symbols {
                    if sym.name.to_lowercase().contains(&query) {
                        #[allow(deprecated)]
                        all_symbols.push(SymbolInformation {
                            name: sym.name,
                            kind: sym.kind,
                            tags: sym.tags,
                            deprecated: sym.deprecated,
                            location: Location {
                                uri: doc.uri.clone(),
                                range: sym.selection_range,
                            },
                            container_name: sym.detail,
                        });
                    }
                }
            }

            if all_symbols.is_empty() {
                Ok(None)
            } else {
                Ok(Some(all_symbols))
            }
        })
        .await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        self.with_tracing("code_action", async move {
            let uri = &params.text_document.uri;
            let mut actions = Vec::new();

            // 1. Diagnostics-based fixes
            for diagnostic in params.context.diagnostics {
                if let Some(NumberOrString::String(ref code)) = diagnostic.code {
                    if code == "unused_fragment" {
                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            uri.clone(),
                            vec![TextEdit {
                                range: diagnostic.range,
                                new_text: String::new(),
                            }],
                        );

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Remove unused fragment".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));

                        if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone()) {
                            let type_only_actions = doc.get_unused_fragment_actions(&diagnostic);
                            for action in type_only_actions {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    } else if code == "unused_variable" {
                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            uri.clone(),
                            vec![TextEdit {
                                range: diagnostic.range,
                                new_text: String::new(),
                            }],
                        );

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Remove unused variable".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));
                    } else if code == "type_only_used" {
                        // The diagnostic may originate from a fragment spread in another file.
                        // We support a quickfix that removes the @type_only directive from the
                        // fragment definition. The diagnostic.data may include the definition
                        // uri and optional def_range for the directive location.
                        let mut target_uri = uri.clone();
                        let mut target_range = diagnostic.range;

                        if let Some(data) = &diagnostic.data {
                            if let Some(def_uri) = data.get("def_uri").and_then(|v| v.as_str())
                                && let Ok(parsed) = Url::parse(def_uri)
                            {
                                target_uri = parsed;
                            }
                            if let Some(def_range) = data.get("def_range")
                                && let Ok(r) = serde_json::from_value::<Range>(def_range.clone())
                            {
                                target_range = r;
                            }
                        }

                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            target_uri.clone(),
                            vec![TextEdit {
                                range: target_range,
                                new_text: String::new(),
                            }],
                        );

                        let mut ca = CodeAction {
                            title: "Remove @type_only directive".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        };

                        // Preserve diagnostic.data so clients can inspect where the definition lives
                        if let Some(d) = &diagnostic.data {
                            ca.data = Some(d.clone());
                        }

                        actions.push(CodeActionOrCommand::CodeAction(ca));
                    } else if code == "missing_field" {
                        if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone()) {
                            let field_actions = doc.get_missing_field_actions(&diagnostic);
                            for action in field_actions {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    } else if code == "no_duplicate_fields" {
                        if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone()) {
                            let actions_for_dup = doc.get_duplicate_field_actions(&diagnostic);
                            for action in actions_for_dup {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    } else if code == "required_field_missing"
                        && let Some(doc) = self.documents.get(uri).map(|r| r.value().clone())
                    {
                        let field_actions = doc.get_required_field_actions(&diagnostic);
                        for action in field_actions {
                            actions.push(CodeActionOrCommand::CodeAction(action));
                        }
                    }
                }
            }

            // 2. Refactoring actions
            if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone()) {
                let schema = self.get_schema_for_doc(uri);
                let refactor_actions = doc.get_extraction_actions(params.range, &schema);
                for action in refactor_actions {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }

                // 3. Format action for inline GraphQL blocks
                if let Some(format_action) = doc.get_format_action(params.range) {
                    actions.push(CodeActionOrCommand::CodeAction(format_action));
                }
            }

            if actions.is_empty() {
                Ok(None)
            } else {
                Ok(Some(actions))
            }
        })
        .await
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.client
            .log_message(MessageType::INFO, "Configuration changed!")
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let start = std::time::Instant::now();

        // Get timeout duration
        let timeout_ms = {
            let config = self.config.read().unwrap();
            config.get_timeouts().lsp_request_ms
        };

        // Apply timeout
        let _res = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async move {
            let config = self.config.read().unwrap().clone();
            for change in params.changes {
                let change_params = super::file_change_handler::FileChangeParams {
                    client: &self.client,
                    config: &config,
                    documents: &self.documents,
                    fragment_defs: &self.fragment_defs,
                    fragment_spreads: &self.fragment_spreads,
                    package_roots: &self.package_roots,
                    fragment_dependents: &self.fragment_dependents,
                    fragment_definitions: &self.fragment_definitions,
                    operation_names: &self.operation_names,
                    gitignore: &self.gitignore,
                };

                let result = if change.typ == FileChangeType::CREATED
                    || change.typ == FileChangeType::CHANGED
                {
                    super::file_change_handler::process_file_created_or_changed(
                        change.uri,
                        &change_params,
                        |uri| self.normalize_uri(uri),
                    )
                    .await
                } else if change.typ == FileChangeType::DELETED {
                    super::file_change_handler::process_file_deleted(
                        change.uri,
                        &change_params,
                        |uri| self.normalize_uri(uri),
                    )
                } else {
                    None
                };

                if let Some(result) = result {
                    // Invalidate fragment metadata cache since fragments might have changed
                    self.invalidate_fragment_cache();

                    // Config reload takes precedence - if config changed, reload everything
                    if result.should_reload_config {
                        self.reload_config().await;
                        continue; // Skip other processing since we're doing a full reload
                    }

                    if result.should_reload_schema
                        && let Some(schema_path) = result.schema_path
                    {
                        self.reload_schema(&schema_path).await;
                    }

                    if !result.uris_to_validate.is_empty() {
                        self.validate_uris(result.uris_to_validate).await;
                    }

                    // Request throttled codegen if enabled
                    if result.should_run_codegen
                        && let Some(throttle) = &self.codegen_throttle
                    {
                        throttle.request_codegen();
                    }
                }
            }
        })
        .await;

        if _res.is_err() {
            let elapsed = start.elapsed();
            self.client
                .log_message(
                    MessageType::ERROR,
                    format!(
                        "LSP Request 'did_change_watched_files' exceeded timeout of {}ms (took {}ms)",
                        timeout_ms,
                        elapsed.as_millis()
                    ),
                )
                .await;
        }

        // Extract tracing config
        let should_log = {
            let config = self.config.read().unwrap();
            config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
        };

        if let Some((enabled, threshold_ms)) = should_log
            && enabled
        {
            let elapsed = start.elapsed();
            if elapsed.as_millis() >= threshold_ms as u128 {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "LSP Request 'did_change_watched_files' took {}ms",
                            elapsed.as_millis()
                        ),
                    )
                    .await;
            }
        }
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        self.with_tracing("execute_command", async move {
            match params.command.as_str() {
                "graphql.runCodegen" => {
                    self.client
                        .log_message(MessageType::INFO, "Running codegen...")
                        .await;
                    self.run_codegen().await;
                    self.client
                        .log_message(MessageType::INFO, "Codegen complete!")
                        .await;
                    Ok(None)
                }
                "graphql.clearCache" => {
                    self.clear_cache().await;
                    Ok(None)
                }
                _ => Ok(None),
            }
        })
        .await
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let start = std::time::Instant::now();

        // Get timeout duration
        let timeout_ms = {
            let config = self.config.read().unwrap();
            config.get_timeouts().lsp_request_ms
        };

        // Apply timeout
        let res = match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            async move {
                let uri = self.normalize_uri(params.text_document.uri.clone());

                // Get the current document version
                let doc_version = if let Some(doc) = self.documents.get(&uri) {
                    doc.version
                } else {
                    // Document not found
                    return Ok(DocumentDiagnosticReportResult::Report(
                        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                            related_documents: None,
                            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                result_id: None,
                                items: vec![],
                            },
                        }),
                    ));
                };

                // Check if we have cached diagnostics
                if let Some(cached) = self.diagnostic_cache.get(&uri) {
                    let (cached_version, cached_diagnostics) = cached.value();

                    // If the cached version matches the previous result ID, return unchanged
                    if let Some(prev_result_id) = &params.previous_result_id
                        && let Ok(prev_version) = prev_result_id.parse::<i32>()
                        && prev_version == *cached_version
                        && prev_version == doc_version
                    {
                        return Ok(DocumentDiagnosticReportResult::Report(
                            DocumentDiagnosticReport::Unchanged(
                                RelatedUnchangedDocumentDiagnosticReport {
                                    related_documents: None,
                                    unchanged_document_diagnostic_report:
                                        UnchangedDocumentDiagnosticReport {
                                            result_id: cached_version.to_string(),
                                        },
                                },
                            ),
                        ));
                    }

                    // Return cached diagnostics if version matches
                    if *cached_version == doc_version {
                        return Ok(DocumentDiagnosticReportResult::Report(
                            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                                related_documents: None,
                                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                    result_id: Some(cached_version.to_string()),
                                    items: cached_diagnostics.clone(),
                                },
                            }),
                        ));
                    }
                }

                // No cache or outdated cache - compute diagnostics
                // Force validation with caching but no push
                self.validate_uris(vec![uri.clone()]).await;

                // Retrieve from cache
                if let Some(cached) = self.diagnostic_cache.get(&uri) {
                    let (version, diagnostics) = cached.value();
                    return Ok(DocumentDiagnosticReportResult::Report(
                        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                            related_documents: None,
                            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                result_id: Some(version.to_string()),
                                items: diagnostics.clone(),
                            },
                        }),
                    ));
                }

                // Fallback: return empty diagnostics
                Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                        related_documents: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: Some(doc_version.to_string()),
                            items: vec![],
                        },
                    }),
                ))
            },
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let elapsed = start.elapsed();
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "LSP Request 'diagnostic' exceeded timeout of {}ms (took {}ms) - returning empty response",
                            timeout_ms,
                            elapsed.as_millis()
                        ),
                    )
                    .await;
                // Return empty diagnostic report on timeout
                Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                        related_documents: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: None,
                            items: vec![],
                        },
                    }),
                ))
            }
        };

        // Extract tracing config
        let should_log = {
            let config = self.config.read().unwrap();
            config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
        };

        if let Some((enabled, threshold_ms)) = should_log
            && enabled
        {
            let elapsed = start.elapsed();
            if elapsed.as_millis() >= threshold_ms as u128 {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("LSP Request 'diagnostic' took {}ms", elapsed.as_millis()),
                    )
                    .await;
            }
        }
        res
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        let start = std::time::Instant::now();

        // Get timeout duration
        let timeout_ms = {
            let config = self.config.read().unwrap();
            config.get_timeouts().lsp_request_ms
        };

        // Apply timeout
        let res = match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            async move {
                let mut items = Vec::new();

                // Get all document URIs
                let all_uris: Vec<Url> = self.documents.iter().map(|e| e.key().clone()).collect();

                // Validate all documents (this will cache diagnostics)
                self.validate_all_documents().await;

                // Collect diagnostics from cache
                for uri in all_uris {
                    if let Some(cached) = self.diagnostic_cache.get(&uri) {
                        let (version, diagnostics) = cached.value();

                        // Check if this URI was in the previous result
                        let unchanged = params
                            .previous_result_ids
                            .iter()
                            .any(|prev| prev.uri == uri && prev.value == version.to_string());

                        if unchanged {
                            items.push(WorkspaceDocumentDiagnosticReport::Unchanged(
                                WorkspaceUnchangedDocumentDiagnosticReport {
                                    uri: uri.clone(),
                                    version: Some((*version) as i64),
                                    unchanged_document_diagnostic_report:
                                        UnchangedDocumentDiagnosticReport {
                                            result_id: version.to_string(),
                                        },
                                },
                            ));
                        } else {
                            items.push(WorkspaceDocumentDiagnosticReport::Full(
                                WorkspaceFullDocumentDiagnosticReport {
                                    uri: uri.clone(),
                                    version: Some((*version) as i64),
                                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                        result_id: Some(version.to_string()),
                                        items: diagnostics.clone(),
                                    },
                                },
                            ));
                        }
                    }
                }

                Ok(WorkspaceDiagnosticReportResult::Report(
                    WorkspaceDiagnosticReport { items },
                ))
            },
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let elapsed = start.elapsed();
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "LSP Request 'workspace_diagnostic' exceeded timeout of {}ms (took {}ms) - returning empty response",
                            timeout_ms,
                            elapsed.as_millis()
                        ),
                    )
                    .await;
                // Return empty workspace diagnostic report on timeout
                Ok(WorkspaceDiagnosticReportResult::Report(
                    WorkspaceDiagnosticReport { items: vec![] },
                ))
            }
        };

        // Extract tracing config
        let should_log = {
            let config = self.config.read().unwrap();
            config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
        };

        if let Some((enabled, threshold_ms)) = should_log
            && enabled
        {
            let elapsed = start.elapsed();
            if elapsed.as_millis() >= threshold_ms as u128 {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "LSP Request 'workspace_diagnostic' took {}ms",
                            elapsed.as_millis()
                        ),
                    )
                    .await;
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphox_core::config::{GlobPattern as GqlGlobPattern, ProjectConfig, SchemaSource};
    use tokio::time::{Duration, timeout};
    use tower_lsp::LspService;

    #[tokio::test]
    async fn test_validate_all_documents_performance() {
        let config = Config {
            base_dir: std::env::current_dir().unwrap(),
            projects: vec![ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GqlGlobPattern::Single("**/*.graphql".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };

        let (service, _) = LspService::new(|client| Backend::new(client, config));

        // This should complete very quickly even with multiple documents
        let res = timeout(
            Duration::from_millis(500),
            service.inner().validate_all_documents(),
        )
        .await;
        assert!(
            res.is_ok(),
            "validate_all_documents took too long or deadlocked"
        );
    }

    #[tokio::test]
    async fn test_get_all_fragments_info_no_deadlock() {
        let config = Config {
            base_dir: std::env::current_dir().unwrap(),
            projects: vec![],
            ..Default::default()
        };

        let (service, _) = LspService::new(|client| Backend::new(client, config));
        let backend = service.inner();

        // Simulate some data
        let uri = Url::parse("file:///test.graphql").unwrap();
        backend.fragment_defs.insert(uri.clone(), vec![]);

        let res = timeout(Duration::from_millis(100), async {
            backend.get_all_fragments_info()
        })
        .await;
        assert!(res.is_ok(), "get_all_fragments_info deadlocked");
    }
}

pub async fn run_lsp(config: Config) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend::new(client, config));
    Server::new(stdin, stdout, socket).serve(service).await;
}
