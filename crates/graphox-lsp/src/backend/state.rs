use graphox_core::config::SchemaSource;
use graphox_core::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap,
    FragmentDependentsMap, FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use graphox_core::{Config, DocumentState};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::definition::DocumentDefinition;

use ahash::{AHashMap, AHashSet};
use apollo_compiler::Schema;
use dashmap::DashMap;
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tower_lsp::{Client, jsonrpc::Result, lsp_types::*};

// Re-export ClientCapabilities for backward compatibility
pub use super::capabilities::ClientCapabilities;

#[derive(Clone)]
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
    /// Tracks if codegen was requested during workspace scan (to run after scan completes)
    pub codegen_requested_during_scan: Arc<AtomicBool>,
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
    pub fn new(client: Client, config: Config) -> Self {
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
        for project in config.projects() {
            let key = project.schema().as_key();
            if !schemas.contains_key(&key) {
                match Self::load_schema_source(config.base_dir(), project.schema()) {
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

        let gitignore = Arc::new(graphox_core::utils::get_gitignore_matcher(
            config.base_dir(),
        ));

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
            codegen_requested_during_scan: Arc::new(AtomicBool::new(false)),
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
        super::helpers::normalize_uri(uri)
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
        let metadata = super::fragment_manager::collect_fragment_metadata(
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
        super::validation::get_fragments_for_doc_with_metadata(
            doc.package_root.as_deref(),
            all_fragments,
        )
    }

    pub fn get_transitive_fragments(
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

    pub async fn with_tracing<T, Fut>(&self, name: &str, fut: Fut) -> Result<Option<T>>
    where
        Fut: std::future::Future<Output = Result<Option<T>>>,
    {
        let timeout_ms = {
            let config = self.config.read().unwrap();
            config.get_timeouts().lsp_request_ms()
        };

        let tracing_config = {
            let config = self.config.read().unwrap();
            let t = config.tracing();
            Some((t.enabled(), t.threshold_ms()))
        };

        super::helpers::with_tracing(&self.client, name, timeout_ms, tracing_config, fut).await
    }

    pub async fn reload_schema(&self, changed_path: &str) {
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
        let (supports_progress, position_encoding) =
            if let Ok(caps) = self.client_capabilities.read() {
                (caps.supports_progress, caps.negotiated_encoding())
            } else {
                (false, PositionEncodingKind::UTF16)
            };

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
            codegen_requested_during_scan: self.codegen_requested_during_scan.clone(),
            trigger_codegen_after_scan: None,
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            validated_schemas: self.validated_schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            codegen_throttle: self.codegen_throttle.clone(),
            supports_progress,
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
            position_encoding,
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

    pub async fn reload_config(&self) {
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
        let base_dir = self.config.read().unwrap().base_dir().to_path_buf();

        // Try to load new config
        let new_config = match Config::load_from_dir(&base_dir) {
            Ok(Some(config)) => config,
            Ok(None) => {
                self.client
                    .log_message(MessageType::ERROR, "Failed to reload configuration file")
                    .await;
                return;
            }
            Err((path, error)) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Failed to parse configuration file {}: {}",
                            path.display(),
                            error
                        ),
                    )
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
        let (supports_progress, position_encoding) =
            if let Ok(caps) = self.client_capabilities.read() {
                (caps.supports_progress, caps.negotiated_encoding())
            } else {
                (false, PositionEncodingKind::UTF16)
            };

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
            codegen_requested_during_scan: self.codegen_requested_during_scan.clone(),
            trigger_codegen_after_scan: None,
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            validated_schemas: self.validated_schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            codegen_throttle: self.codegen_throttle.clone(),
            supports_progress,
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
            position_encoding,
        });

        self.client
            .log_message(MessageType::INFO, "Configuration reloaded successfully")
            .await;
    }

    pub fn update_dependency_indices(
        &self,
        uri: &Url,
        old_spreads: Option<Vec<Arc<str>>>,
        new_spreads: Vec<Arc<str>>,
    ) {
        super::fragment_manager::update_fragment_dependents(
            &self.fragment_dependents,
            uri,
            old_spreads,
            new_spreads,
        );
    }

    pub fn update_definition_indices(
        &self,
        uri: &Url,
        old_fragments: Option<Vec<Arc<str>>>,
        new_fragments: Vec<Arc<str>>,
    ) {
        super::fragment_manager::update_fragment_definitions(
            &self.fragment_definitions,
            uri,
            old_fragments,
            new_fragments,
        );
    }

    pub async fn validate_uris(&self, uris: Vec<Url>) {
        let (use_push, supports_progress, position_encoding) =
            if let Ok(caps) = self.client_capabilities.read() {
                (
                    !caps.supports_pull_diagnostics,
                    caps.supports_progress,
                    caps.negotiated_encoding(),
                )
            } else {
                (true, false, PositionEncodingKind::UTF16) // Default if can't read capabilities
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
            position_encoding,
        };
        super::validation::validate_uris(params, uris, use_push, Some(&self.diagnostic_cache))
            .await;
    }

    pub async fn validate_all_documents(&self) {
        let (use_push, supports_progress, position_encoding) =
            if let Ok(caps) = self.client_capabilities.read() {
                (
                    !caps.supports_pull_diagnostics,
                    caps.supports_progress,
                    caps.negotiated_encoding(),
                )
            } else {
                (true, false, PositionEncodingKind::UTF16) // Default if can't read capabilities
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
            position_encoding,
        };
        super::validation::validate_all_documents(params, use_push, Some(&self.diagnostic_cache))
            .await;
    }

    pub fn get_affected_uris(
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
    pub fn get_preferred_schema_uris(&self, uri: &Url) -> Vec<Url> {
        let mut preferred_uris = Vec::new();
        if let Ok(path) = uri.to_file_path() {
            let config = self.config.read().unwrap();
            if let Some(project) = config.get_project_for_path(&path) {
                for schema_file in project.schema().files() {
                    let schema_path = config.base_dir().join(schema_file);
                    if let Ok(schema_uri) = Url::from_file_path(schema_path) {
                        preferred_uris.push(schema_uri);
                    }
                }
            }
        }
        preferred_uris
    }

    /// Try to find fragment definition location
    pub async fn try_goto_fragment_definition(
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
    pub fn lookup_fragment_in_index(
        &self,
        name: &str,
        doc: &Arc<DocumentState>,
    ) -> Option<Location> {
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
    pub async fn scan_all_documents_for_fragment(
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

        let name = name.to_string();
        let doc = doc.clone();
        let backend = self.clone();

        tokio::task::spawn_blocking(move || {
            doc_arcs.par_iter().find_map_any(|other_doc| {
                if backend.is_fragment_accessible(other_doc, &doc, &name) {
                    other_doc.find_definition_in_tree(&name)
                } else {
                    None
                }
            })
        })
        .await
        .unwrap()
    }

    /// Check if a fragment is accessible from the current document
    pub fn is_fragment_accessible(
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
