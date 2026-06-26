use graphox_core::config::SchemaSource;
use graphox_core::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDependentsMap, MetadataMap,
    OperationNamesMap,
};
use graphox_core::{Config, DocumentState};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::definition::DocumentDefinition;

use ahash::{AHashMap, AHashSet};
use apollo_compiler::Schema;
use dashmap::DashMap;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use tower_lsp::{Client, jsonrpc::Result, lsp_types::*};

// Re-export ClientCapabilities for backward compatibility
pub use super::capabilities::ClientCapabilities;

const WORKSPACE_DIAGNOSTIC_REFRESH_WARNING_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(500);

type ConfiguredDocumentUrisSnapshot = (usize, Arc<Vec<Url>>);
type ConfiguredDocumentUrisCache = Arc<std::sync::RwLock<Option<ConfiguredDocumentUrisSnapshot>>>;

pub(crate) fn spawn_workspace_diagnostic_refresh(client: Client) {
    tokio::spawn(async move {
        let refresh_request = client.workspace_diagnostic_refresh();
        tokio::pin!(refresh_request);

        match tokio::time::timeout(
            WORKSPACE_DIAGNOSTIC_REFRESH_WARNING_TIMEOUT,
            &mut refresh_request,
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                client
                    .log_message(
                        MessageType::WARNING,
                        format!("workspace/diagnostic/refresh failed: {err}"),
                    )
                    .await;
            }
            Err(_) => {
                client
                    .log_message(
                        MessageType::WARNING,
                        "workspace/diagnostic/refresh is taking longer than expected",
                    )
                    .await;

                if let Err(err) = refresh_request.await {
                    client
                        .log_message(
                            MessageType::WARNING,
                            format!("workspace/diagnostic/refresh failed: {err}"),
                        )
                        .await;
                }
            }
        }
    });
}

#[derive(Clone)]
pub struct Backend {
    pub client: Client,
    pub documents: DocumentsMap,
    pub config: Arc<std::sync::RwLock<Config>>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub subgraphs:
        Arc<DashMap<String, Vec<graphox_core::schema::SubgraphInfo>, ahash::RandomState>>,
    pub empty_schema: Arc<Schema>,
    pub valid_empty_schema: Arc<apollo_compiler::validation::Valid<Schema>>,
    pub validated_schemas:
        Arc<DashMap<String, Arc<apollo_compiler::validation::Valid<Schema>>, ahash::RandomState>>,
    // Metadata for all files in workspace
    pub metadata: MetadataMap,
    pub fragment_dependents: FragmentDependentsMap,
    pub fragment_definitions: FragmentDefinitionsMap,
    /// Maps operation name -> (project schema key, URI)
    /// Used to detect duplicate operation names within a project
    pub operation_names: OperationNamesMap,
    pub workspace_loaded: Arc<AtomicBool>,
    /// Tracks if codegen was requested during workspace scan (to run after scan completes)
    pub codegen_requested_during_scan: Arc<AtomicBool>,
    pub open_documents: Arc<dashmap::DashSet<Url, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<std::sync::RwLock<Arc<AtomicBool>>>,
    pub gitignore: Arc<ignore::gitignore::Gitignore>,
    /// Persistent type cache per schema (keyed by schema key)
    /// Shared across all codegen runs for the same schema to maximize cache hits
    pub type_caches:
        Arc<DashMap<String, Arc<graphox_codegen::SchemaAnalysisCaches>, ahash::RandomState>>,
    /// Client capabilities for conditional feature enablement
    pub client_capabilities: Arc<std::sync::RwLock<ClientCapabilities>>,
    /// Cached diagnostics for pull-based diagnostics (URI -> (version, workspace epoch, diagnostics))
    pub diagnostic_cache: DiagnosticCacheMap,
    /// Current version of the workspace, incremented on any change
    pub workspace_version: Arc<std::sync::atomic::AtomicUsize>,
    /// Version of the workspace when the last full validation was completed
    pub last_full_validation_version: Arc<std::sync::atomic::AtomicUsize>,
    /// Throttled codegen runner
    pub codegen_throttle:
        Arc<std::sync::RwLock<Option<Arc<super::codegen_throttle::CodegenThrottle>>>>,
    /// Global cache for all fragments in the workspace
    pub fragment_metadata_cache: Arc<std::sync::RwLock<Option<Arc<Vec<FragmentCompletionInfo>>>>>,
    /// Reuse cache for the no-SLO fragment list built during validation, keyed by
    /// workspace version. See [`super::validation::ValidationFragmentCache`].
    pub validation_fragment_cache: super::validation::ValidationFragmentCache,
    /// Configured document URIs for the current workspace version
    pub configured_document_uris_cache: ConfiguredDocumentUrisCache,
    /// Weak reference to self for use in background tasks
    pub self_weak: Weak<Backend>,
}

impl Backend {
    pub fn new(client: Client, config: Config) -> Arc<Self> {
        let schemas = Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let subgraphs = Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let validated_schemas = Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let documents: DocumentsMap = Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let metadata: MetadataMap = Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let fragment_dependents: FragmentDependentsMap =
            Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let fragment_definitions: FragmentDefinitionsMap =
            Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let operation_names: OperationNamesMap =
            Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        let diagnostic_cache: DiagnosticCacheMap =
            Arc::new(DashMap::with_hasher(ahash::RandomState::default()));

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
                        schemas.insert(key.clone(), schema);

                        if let Some(subgraphs_dir) = project.subgraphs_dir() {
                            let project_subgraphs = graphox_core::schema::load_subgraphs(
                                config.base_dir(),
                                subgraphs_dir,
                                project.subgraph_owners(),
                            );
                            subgraphs.insert(key, project_subgraphs);
                        }
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
        let workspace_loaded = Arc::new(AtomicBool::new(false));
        let codegen_requested_during_scan = Arc::new(AtomicBool::new(false));
        let open_documents = Arc::new(dashmap::DashSet::with_hasher(ahash::RandomState::default()));
        let workspace_scan_cancelled =
            Arc::new(std::sync::RwLock::new(Arc::new(AtomicBool::new(false))));
        let client_capabilities = Arc::new(std::sync::RwLock::new(ClientCapabilities::default()));
        let workspace_version = Arc::new(std::sync::atomic::AtomicUsize::new(1));
        let last_full_validation_version = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let fragment_metadata_cache = Arc::new(std::sync::RwLock::new(None));
        let validation_fragment_cache = Arc::new(std::sync::RwLock::new(None));
        let configured_document_uris_cache = Arc::new(std::sync::RwLock::new(None));

        Arc::new_cyclic(|this| {
            // Create codegen throttle if automatic codegen is enabled
            let codegen_throttle = Arc::new(std::sync::RwLock::new({
                let cfg = config_arc.read().unwrap();
                if cfg.lsp_automatic_codegen() {
                    Some(Arc::new(super::codegen_throttle::CodegenThrottle::new(
                        this.clone(),
                    )))
                } else {
                    None
                }
            }));

            Self {
                client,
                documents,
                config: config_arc.clone(),

                schemas,
                subgraphs,
                validated_schemas,
                empty_schema,
                valid_empty_schema,
                metadata,
                fragment_dependents,
                fragment_definitions,
                operation_names,
                workspace_loaded,
                codegen_requested_during_scan,
                open_documents,
                workspace_scan_cancelled,
                gitignore,
                type_caches,
                client_capabilities,
                diagnostic_cache,
                workspace_version,
                last_full_validation_version,
                codegen_throttle,
                fragment_metadata_cache,
                validation_fragment_cache,
                configured_document_uris_cache,
                self_weak: this.clone(),
            }
        })
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

    pub fn get_position_encoding(&self) -> PositionEncodingKind {
        if let Ok(caps) = self.client_capabilities.read() {
            caps.negotiated_encoding()
        } else {
            PositionEncodingKind::UTF16
        }
    }

    pub fn increment_workspace_version(&self) {
        self.workspace_version
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn get_configured_document_uris(&self) -> Arc<Vec<Url>> {
        let workspace_epoch = self.workspace_version.load(Ordering::SeqCst);
        if let Ok(cache) = self.configured_document_uris_cache.read()
            && let Some((cached_epoch, uris)) = &*cache
            && *cached_epoch == workspace_epoch
        {
            return uris.clone();
        }

        let config = self.config.read().unwrap().clone();
        let uris: Arc<Vec<Url>> = Arc::new(
            self.documents
                .iter()
                .map(|entry| entry.key().clone())
                .filter(|uri| crate::backend::validation::is_configured_document_uri(uri, &config))
                .collect(),
        );

        if let Ok(mut cache) = self.configured_document_uris_cache.write() {
            *cache = Some((workspace_epoch, uris.clone()));
        }

        uris
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

    pub fn get_all_fragments_info(&self) -> Arc<Vec<FragmentCompletionInfo>> {
        // Try to return cached metadata if available
        if let Ok(cache) = self.fragment_metadata_cache.read()
            && let Some(metadata) = &*cache
        {
            return metadata.clone();
        }

        // Cache miss: collect metadata and update cache
        let config = self.config.read().unwrap();
        // Completion surfaces each fragment's worst-case SLO, so compute it here.
        // The result is cached, so the SLO pass runs at most once per workspace epoch.
        let metadata = Arc::new(super::fragment_manager::collect_fragment_metadata(
            &self.metadata,
            &config,
            &self.subgraphs,
            &self.documents,
            &self.schemas,
            true,
        ));

        if let Ok(mut cache) = self.fragment_metadata_cache.write() {
            *cache = Some(metadata.clone());
        }

        metadata
    }

    pub async fn load_doc_from_cache_or_disk(&self, uri: &Url) -> Option<Arc<DocumentState>> {
        if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone()) {
            return Some(doc);
        }

        let path = uri.to_file_path().ok()?;
        let encoding = self.get_position_encoding();
        let uri_clone = uri.clone();

        tokio::task::spawn_blocking(move || {
            let content = std::fs::read_to_string(&path).ok()?;
            Some(Arc::new(DocumentState::new_from_thread_local(
                uri_clone, &content, encoding,
            )))
        })
        .await
        .ok()
        .flatten()
    }

    pub fn clear_operation_names_for_uri(&self, uri: &Url) {
        let old_operation_names = self
            .metadata
            .get(uri)
            .map(|metadata| super::helpers::named_operation_names(&metadata.operations));
        let config = self.config.read().unwrap().clone();
        super::helpers::update_operation_name_index(
            &self.operation_names,
            &config,
            uri,
            old_operation_names.as_deref(),
            &[],
        );
    }

    pub fn find_type_definition_in_schema(
        &self,
        schema: &Schema,
        type_name: &str,
    ) -> Option<Location> {
        let ty = schema.types.get(type_name)?;
        let name_node = match ty {
            apollo_compiler::schema::ExtendedType::Object(obj) => obj.name.clone(),
            apollo_compiler::schema::ExtendedType::Interface(iface) => iface.name.clone(),
            apollo_compiler::schema::ExtendedType::Enum(enm) => enm.name.clone(),
            apollo_compiler::schema::ExtendedType::Scalar(scal) => scal.name.clone(),
            apollo_compiler::schema::ExtendedType::Union(unn) => unn.name.clone(),
            apollo_compiler::schema::ExtendedType::InputObject(inp) => inp.name.clone(),
        };

        let file_id = name_node.location()?.file_id();
        let source_file = schema.sources.get(&file_id)?;
        let path = source_file.path();
        let uri = Url::from_file_path(path).ok()?;

        let range = graphox_core::utils::apollo_location_to_range(
            &name_node.location(),
            source_file,
            self.get_position_encoding(),
        )?;

        Some(Location { uri, range })
    }

    pub fn invalidate_fragment_cache(&self) {
        if let Ok(mut cache) = self.fragment_metadata_cache.write() {
            *cache = None;
        }
    }

    pub fn refresh_pull_diagnostics_for(&self, source_uri: &Url, uris_to_validate: &[Url]) {
        let supports_pull_diagnostics = self
            .client_capabilities
            .read()
            .map(|caps| caps.supports_pull_diagnostics)
            .unwrap_or(false);

        if !supports_pull_diagnostics
            || !uris_to_validate
                .iter()
                .any(|validated_uri| validated_uri != source_uri)
        {
            return;
        }

        spawn_workspace_diagnostic_refresh(self.client.clone());
    }

    pub fn get_fragments_for_doc(
        &self,
        doc: &DocumentState,
        all_fragments: &[FragmentCompletionInfo],
    ) -> Vec<FragmentCompletionInfo> {
        let mut filtered: Vec<_> = all_fragments
            .iter()
            .filter_map(|f| {
                let accessible = self.is_fragment_accessible(&f.uri, &doc.uri, &f.name);
                if accessible {
                    let is_same_package = graphox_core::utils::paths_match(
                        f.package_root.as_deref(),
                        doc.package_root.as_deref(),
                    );
                    Some((f.clone(), is_same_package))
                } else {
                    None
                }
            })
            .collect();

        // Prioritize fragments from same package
        filtered.sort_by(|(a, a_same_pkg), (b, b_same_pkg)| {
            if a_same_pkg != b_same_pkg {
                return b_same_pkg.cmp(a_same_pkg);
            }
            a.name.cmp(&b.name)
        });

        filtered.into_iter().map(|(f, _)| f).collect()
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
                        for nested in def.used_fragments.iter() {
                            to_visit.push(nested.clone());
                        }
                    }
                }
            }
        }

        fragment_uris
    }

    pub async fn get_fragment_requirements(
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

        let mut stack: Vec<Arc<str>> = vec![Arc::from(name)];

        while let Some(current_name) = stack.pop() {
            if !visited.insert(current_name.clone()) {
                continue;
            }

            if let Some(frag) = all_fragments.iter().find(|f| {
                f.name == current_name
                    && (f.is_public
                        || graphox_core::utils::paths_match(
                            f.package_root.as_deref(),
                            package_root.map(|p| p.as_path()),
                        ))
            }) {
                let doc_arc = self.load_doc_from_cache_or_disk(&frag.uri).await;

                if let Some(doc) = doc_arc {
                    let local_vars = if let Some(cached) = variable_types_cache.get(&current_name) {
                        cached.clone()
                    } else {
                        let vars = doc.get_fragment_variable_types(&current_name, schema);
                        let mut vars_arc = std::collections::BTreeMap::new();
                        for (k, v) in vars {
                            vars_arc.insert(Arc::from(k), Arc::from(v));
                        }
                        variable_types_cache.insert(current_name.clone(), vars_arc.clone());
                        vars_arc
                    };

                    for (k, v) in local_vars {
                        requirements.insert(k, v);
                    }

                    // Add nested fragments
                    if let Some(def) = doc.fragments().iter().find(|f| f.name == current_name) {
                        for nested in def.used_fragments.iter() {
                            stack.push(nested.clone());
                        }
                    }
                }
            }
        }

        requirements
    }

    pub fn get_used_fragments(&self) -> AHashSet<Arc<str>> {
        let config = self.config.read().unwrap();
        super::validation::get_used_fragments(&self.metadata, &config)
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
            &self.subgraphs,
            &self.validated_schemas,
            &self.client,
            supports_progress,
        )
        .await;

        if !reloaded_keys.is_empty() {
            // Invalidate fragment metadata cache since schema changed,
            // which might affect fragment requirements resolution.
            self.invalidate_fragment_cache();
            self.increment_workspace_version();
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
        // Cancel any ongoing scan first
        self.workspace_scan_cancelled
            .read()
            .unwrap()
            .store(true, Ordering::SeqCst);

        // Collect currently open documents to restore them after clearing
        let open_docs: Vec<(Url, Arc<DocumentState>)> = self
            .documents
            .iter()
            .filter(|entry| self.open_documents.contains(entry.key()))
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        // Clear fragment metadata cache
        self.invalidate_fragment_cache();

        // Clear globset cache in config
        graphox_core::config::clear_globset_cache();

        // Clear all internal state
        self.schemas.clear();
        self.subgraphs.clear();
        self.validated_schemas.clear();
        let _ = graphox_core::schema_cache::clear_cache();

        // Reset cancellation flag for the new scan we're about to spawn
        *self.workspace_scan_cancelled.write().unwrap() = Arc::new(AtomicBool::new(false));

        // Clear all documents (including open ones) to ensure full re-load
        self.documents.clear();
        self.metadata.clear();

        self.fragment_dependents.clear();
        self.fragment_definitions.clear();
        self.type_caches.clear();
        self.diagnostic_cache.clear();
        self.operation_names.clear();

        let new_config = self.config.read().unwrap().clone();

        // Populate subgraphs for relevant projects first so validation is subgraph-aware
        for project in new_config.projects() {
            if let Some(subgraphs_dir) = project.subgraphs_dir() {
                let subgraphs = graphox_core::schema::load_subgraphs(
                    new_config.base_dir(),
                    subgraphs_dir,
                    project.subgraph_owners(),
                );
                self.subgraphs.insert(project.schema().as_key(), subgraphs);
            }
        }

        // Pre-load schemas for open documents to ensure immediate validation is correct
        for (uri, _) in &open_docs {
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = new_config.get_schema_for_path(&path)
                && !self.schemas.contains_key(&schema_key)
                && let Some(project) = new_config
                    .projects()
                    .iter()
                    .find(|p| p.schema().as_key() == schema_key)
                && let Ok(schema) = graphox_core::schema::load_schema_with_cache(
                    new_config.base_dir(),
                    project.schema(),
                    false, // DISABLE CACHE
                )
            {
                self.schemas
                    .insert(schema_key.clone(), Arc::new(schema.clone()));
                if let Ok(valid) = schema.validate() {
                    self.validated_schemas.insert(schema_key, Arc::new(valid));
                }
            }
        }

        // Restore open documents AND re-index them
        let mut uris_to_validate = Vec::new();
        for (uri, doc) in open_docs {
            self.documents.insert(uri.clone(), doc.clone());
            uris_to_validate.push(uri.clone());

            // Re-index
            let metadata = Arc::new(graphox_core::types::DocumentMetadata {
                fragments: doc.fragments.clone(),
                fragment_spreads: doc.fragment_spreads.clone(),
                package_root: doc.package_root.clone(),
                operations: doc.operations.clone(),
                version: doc.version,
            });
            self.metadata.insert(uri.clone(), metadata);

            // Re-populate operation names index
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = new_config.get_schema_for_path(&path)
            {
                let project_key = new_config
                    .get_project_for_path(&path)
                    .map(|p| p.include().as_key())
                    .unwrap_or_else(|| schema_key);
                let project_key_arc: Arc<str> = project_key.into();

                for op in doc.operations.iter() {
                    if let Some(name) = &op.name {
                        self.operation_names
                            .entry(name.clone())
                            .or_default()
                            .push((project_key_arc.clone(), uri.clone()));
                    }
                }
            }

            self.update_dependency_indices(&uri, None, doc.fragment_spreads.clone());
            self.update_definition_indices(
                &uri,
                None,
                doc.fragments
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<Arc<[_]>>(),
            );
        }

        // Trigger immediate validation of open documents
        if !uris_to_validate.is_empty() {
            self.validate_uris(uris_to_validate).await;
        }

        // Reset workspace version
        self.increment_workspace_version();
        self.last_full_validation_version.store(0, Ordering::SeqCst);

        // Trigger workspace scan to re-index everything
        let (supports_progress, position_encoding, supports_pull_diagnostics) =
            if let Ok(caps) = self.client_capabilities.read() {
                (
                    caps.supports_progress,
                    caps.negotiated_encoding(),
                    caps.supports_pull_diagnostics,
                )
            } else {
                (false, PositionEncodingKind::UTF16, false)
            };

        // Reset workspace_loaded flag
        self.workspace_loaded.store(false, Ordering::Relaxed);

        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config: new_config,
            supports_pull_diagnostics,
            documents: self.documents.clone(),
            metadata: self.metadata.clone(),
            fragment_dependents: self.fragment_dependents.clone(),
            fragment_definitions: self.fragment_definitions.clone(),
            operation_names: self.operation_names.clone(),
            workspace_loaded: self.workspace_loaded.clone(),
            codegen_requested_during_scan: self.codegen_requested_during_scan.clone(),
            trigger_codegen_after_scan: None,
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            subgraphs: self.subgraphs.clone(),
            validated_schemas: self.validated_schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.read().unwrap().clone(),
            codegen_throttle: self.codegen_throttle.clone(),
            supports_progress,
            bypass_cache: true,
            diagnostic_cache: self.diagnostic_cache.clone(),
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
            position_encoding,
            workspace_version: self.workspace_version.clone(),
            last_full_validation_version: self.last_full_validation_version.clone(),
            open_documents: self.open_documents.clone(),
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
            self.documents.clone(),
            supports_progress,
            None,
            self.get_position_encoding(),
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

        // Collect currently open documents to restore them after clearing
        let open_docs: Vec<(Url, Arc<DocumentState>)> = self
            .documents
            .iter()
            .filter(|entry| self.open_documents.contains(entry.key()))
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        // Update the config
        *self.config.write().unwrap() = new_config.clone();

        // Cancel any active scan and reset workspace loaded state
        {
            let mut cancelled_lock = self.workspace_scan_cancelled.write().unwrap();
            cancelled_lock.store(true, Ordering::SeqCst);
            *cancelled_lock = Arc::new(AtomicBool::new(false));
        }
        self.workspace_loaded.store(false, Ordering::SeqCst);

        // Update codegen throttle
        {
            let mut throttle_lock = self.codegen_throttle.write().unwrap();
            if new_config.lsp_automatic_codegen() {
                if throttle_lock.is_none() {
                    *throttle_lock = Some(Arc::new(super::codegen_throttle::CodegenThrottle::new(
                        self.self_weak.clone(),
                    )));
                }
            } else {
                *throttle_lock = None;
            }
        }

        // Clear all state
        self.schemas.clear();
        self.subgraphs.clear();
        self.validated_schemas.clear();
        let _ = graphox_core::schema_cache::clear_cache();

        // Clear only what's necessary - we re-index everything to match the new config.
        self.documents.clear();
        self.metadata.clear();

        self.fragment_dependents.clear();
        self.fragment_definitions.clear();
        self.type_caches.clear();
        self.diagnostic_cache.clear();
        self.operation_names.clear();

        // Populate subgraphs for relevant projects first so validation is subgraph-aware
        for project in new_config.projects() {
            if let Some(subgraphs_dir) = project.subgraphs_dir() {
                let subgraphs = graphox_core::schema::load_subgraphs(
                    new_config.base_dir(),
                    subgraphs_dir,
                    project.subgraph_owners(),
                );
                self.subgraphs.insert(project.schema().as_key(), subgraphs);
            }
        }

        // Pre-load all project schemas to ensure immediate validation is correct for any document
        for project in new_config.projects() {
            let schema_key = project.schema().as_key();
            if !self.schemas.contains_key(&schema_key)
                && let Ok(schema) = graphox_core::schema::load_schema_with_cache(
                    new_config.base_dir(),
                    project.schema(),
                    new_config.enable_schema_cache(),
                )
            {
                self.schemas
                    .insert(schema_key.clone(), Arc::new(schema.clone()));
                if let Ok(valid) = schema.validate() {
                    self.validated_schemas.insert(schema_key, Arc::new(valid));
                }
            }
        }

        // Restore open documents AND re-index them
        let mut uris_to_validate = Vec::new();
        for (uri, doc) in open_docs {
            self.documents.insert(uri.clone(), doc.clone());
            uris_to_validate.push(uri.clone());

            // Re-index
            let metadata = Arc::new(graphox_core::types::DocumentMetadata {
                fragments: doc.fragments.clone(),
                fragment_spreads: doc.fragment_spreads.clone(),
                package_root: doc.package_root.clone(),
                operations: doc.operations.clone(),
                version: doc.version,
            });
            self.metadata.insert(uri.clone(), metadata);

            // Re-populate operation names index
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = new_config.get_schema_for_path(&path)
            {
                let project_key = new_config
                    .get_project_for_path(&path)
                    .map(|p| p.include().as_key())
                    .unwrap_or_else(|| schema_key);
                let project_key_arc: Arc<str> = project_key.into();

                for op in doc.operations.iter() {
                    if let Some(name) = &op.name {
                        self.operation_names
                            .entry(name.clone())
                            .or_default()
                            .push((project_key_arc.clone(), uri.clone()));
                    }
                }
            }

            self.update_dependency_indices(&uri, None, doc.fragment_spreads.clone());
            self.update_definition_indices(
                &uri,
                None,
                doc.fragments
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<Arc<[_]>>(),
            );
        }

        // Trigger immediate validation of open documents
        if !uris_to_validate.is_empty() {
            self.validate_uris(uris_to_validate).await;
        }

        // Reset workspace version
        self.increment_workspace_version();
        self.last_full_validation_version.store(0, Ordering::SeqCst);

        // Trigger workspace scan to re-index everything
        let (supports_progress, position_encoding, supports_pull_diagnostics) =
            if let Ok(caps) = self.client_capabilities.read() {
                (
                    caps.supports_progress,
                    caps.negotiated_encoding(),
                    caps.supports_pull_diagnostics,
                )
            } else {
                (false, PositionEncodingKind::UTF16, false)
            };

        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config: new_config,
            supports_pull_diagnostics,
            documents: self.documents.clone(),
            metadata: self.metadata.clone(),
            fragment_dependents: self.fragment_dependents.clone(),
            fragment_definitions: self.fragment_definitions.clone(),
            operation_names: self.operation_names.clone(),
            workspace_loaded: self.workspace_loaded.clone(),
            codegen_requested_during_scan: self.codegen_requested_during_scan.clone(),
            trigger_codegen_after_scan: {
                let throttle_handle = self.codegen_throttle.clone();
                Some(Arc::new(move || {
                    if let Some(throttle) = throttle_handle.read().unwrap().as_ref() {
                        throttle.request_codegen(None);
                    }
                }) as Arc<dyn Fn() + Send + Sync>)
            },
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            subgraphs: self.subgraphs.clone(),
            validated_schemas: self.validated_schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.read().unwrap().clone(),
            codegen_throttle: self.codegen_throttle.clone(),
            supports_progress,
            bypass_cache: false,
            diagnostic_cache: self.diagnostic_cache.clone(),
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
            position_encoding,
            workspace_version: self.workspace_version.clone(),
            last_full_validation_version: self.last_full_validation_version.clone(),
            open_documents: self.open_documents.clone(),
        });

        self.client
            .log_message(MessageType::INFO, "Configuration reloaded successfully")
            .await;
    }

    pub fn update_dependency_indices(
        &self,
        uri: &Url,
        old_spreads: Option<Arc<[Arc<str>]>>,
        new_spreads: Arc<[Arc<str>]>,
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
        old_fragments: Option<Arc<[Arc<str>]>>,
        new_fragments: Arc<[Arc<str>]>,
    ) {
        super::fragment_manager::update_fragment_definitions(
            &self.fragment_definitions,
            uri,
            old_fragments,
            new_fragments,
        );
    }

    pub async fn validate_uris(&self, uris: Vec<Url>) {
        let config = self.config.read().unwrap().clone();

        // Ensure schemas for these URIs are loaded
        for uri in &uris {
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = config.get_schema_for_path(&path)
                && !self.validated_schemas.contains_key(&schema_key)
                && let Some(project) = config
                    .projects()
                    .iter()
                    .find(|p| p.schema().as_key() == schema_key)
                && let Ok(schema) = graphox_core::schema::load_schema_with_cache(
                    config.base_dir(),
                    project.schema(),
                    config.enable_schema_cache(),
                )
            {
                self.schemas
                    .insert(schema_key.clone(), Arc::new(schema.clone()));
                if let Ok(valid) = schema.validate() {
                    self.validated_schemas.insert(schema_key, Arc::new(valid));
                }
            }
        }

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

        let params = super::validation::ValidationParams {
            client: &self.client,
            documents: &self.documents,
            metadata: &self.metadata,
            config: &config,
            validated_schemas: &self.validated_schemas,
            valid_empty_schema: &self.valid_empty_schema,
            workspace_loaded: &self.workspace_loaded,
            open_documents: &self.open_documents,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
            operation_names: &self.operation_names,
            subgraphs: &self.subgraphs,
            schemas: &self.schemas,
            supports_progress,
            position_encoding,
            result_id_epoch: self.workspace_version.load(Ordering::SeqCst),
            validation_fragment_cache: Some(&self.validation_fragment_cache),
        };
        super::validation::validate_uris(params, uris, use_push, Some(&self.diagnostic_cache))
            .await;
    }

    pub async fn validate_all_documents(&self) {
        let current_version = self.workspace_version.load(Ordering::SeqCst);
        let last_version = self.last_full_validation_version.load(Ordering::SeqCst);

        if last_version >= current_version {
            return;
        }

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
            metadata: &self.metadata,
            config: &config,
            validated_schemas: &self.validated_schemas,
            valid_empty_schema: &self.valid_empty_schema,
            workspace_loaded: &self.workspace_loaded,
            open_documents: &self.open_documents,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
            operation_names: &self.operation_names,
            subgraphs: &self.subgraphs,
            schemas: &self.schemas,
            supports_progress,
            position_encoding,
            result_id_epoch: current_version,
            validation_fragment_cache: Some(&self.validation_fragment_cache),
        };

        let uris: Vec<Url> = self
            .documents
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        super::validation::validate_uris(params, uris, use_push, Some(&self.diagnostic_cache))
            .await;

        self.last_full_validation_version
            .store(current_version, Ordering::SeqCst);
    }

    pub fn get_affected_uris(
        &self,
        initial_uri: Url,
        affected_fragment_names: AHashSet<Arc<str>>,
        affected_spread_names: AHashSet<Arc<str>>,
        affected_operation_names: AHashSet<Arc<str>>,
    ) -> Vec<Url> {
        super::validation::get_affected_uris(
            initial_uri,
            affected_fragment_names,
            affected_spread_names,
            affected_operation_names,
            &self.documents,
            &self.fragment_dependents,
            &self.fragment_definitions,
            &self.operation_names,
        )
    }

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

    pub async fn try_goto_fragment_definition(
        &self,
        symbol_name: &Option<String>,
        doc: &Arc<DocumentState>,
    ) -> Option<Location> {
        let name = symbol_name.as_ref()?;

        if let Some(location) = self.lookup_fragment_in_index(name, doc) {
            return Some(location);
        }

        self.scan_all_documents_for_fragment(name, doc).await
    }

    pub fn lookup_fragment_in_index(
        &self,
        name: &str,
        doc: &Arc<DocumentState>,
    ) -> Option<Location> {
        let uris = self.fragment_definitions.get(name)?;
        let mut potentials = Vec::new();
        for other_uri in uris.iter() {
            if let Some(other_doc) = self.documents.get(other_uri).map(|r| r.value().clone()) {
                let is_same_pkg = graphox_core::utils::paths_match(
                    other_doc.package_root.as_deref(),
                    doc.package_root.as_deref(),
                );
                let is_pub = other_doc
                    .fragments()
                    .iter()
                    .any(|f| f.name.as_ref() == name && f.is_public);
                if is_same_pkg || is_pub {
                    potentials.push((is_same_pkg, is_pub, other_doc));
                }
            }
        }
        potentials.sort_by(|a, b| {
            if a.0 != b.0 {
                return b.0.cmp(&a.0);
            }
            b.1.cmp(&a.1)
        });
        if let Some((_, _, best_doc)) = potentials.first() {
            return best_doc.find_definition_in_tree(name);
        }
        None
    }

    pub async fn scan_all_documents_for_fragment(
        &self,
        name: &str,
        doc: &Arc<DocumentState>,
    ) -> Option<Location> {
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
                if backend.is_fragment_accessible(&other_doc.uri, &doc.uri, &name) {
                    other_doc.find_definition_in_tree(&name)
                } else {
                    None
                }
            })
        })
        .await
        .unwrap()
    }

    pub fn is_fragment_accessible(
        &self,
        fragment_doc_uri: &Url,
        target_doc_uri: &Url,
        fragment_name: &str,
    ) -> bool {
        let fragment_meta = match self.metadata.get(fragment_doc_uri) {
            Some(m) => m,
            None => return false,
        };

        // If in same package, it's accessible (private fragments allowed)
        let target_meta = self.metadata.get(target_doc_uri);
        let is_same_package = graphox_core::utils::paths_match(
            fragment_meta.package_root.as_deref(),
            target_meta.as_ref().and_then(|m| m.package_root.as_deref()),
        );

        let is_public_fragment = fragment_meta
            .fragments
            .iter()
            .any(|f| f.name.as_ref() == fragment_name && f.is_public);

        if is_same_package || is_public_fragment {
            return true;
        }

        // CHECK PROJECT IMPORTS
        if let (Ok(f_path), Ok(t_path)) = (
            fragment_doc_uri.to_file_path(),
            target_doc_uri.to_file_path(),
        ) {
            let config = self.config.read().unwrap();
            if let (Some(f_proj), Some(t_project)) = (
                config.get_project_for_path(&f_path),
                config.get_project_for_path(&t_path),
            ) {
                let f_import = f_proj.import();
                if let Some(import_name) = f_import {
                    return t_project.imports().iter().any(|i| i == import_name);
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphox_core::config::{GlobPattern as GqlGlobPattern, ProjectConfig, SchemaSource};
    use std::fs;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower_lsp::LspService;

    #[tokio::test]
    async fn configured_document_uris_cache_reuses_results_until_workspace_changes() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("graphox-configured-uris-{unique}"));
        fs::create_dir_all(&base_dir).unwrap();

        let schema_path = base_dir.join("schema.graphql");
        let query_path = base_dir.join("query.graphql");
        fs::write(&schema_path, "type Query { hello: String }").unwrap();
        fs::write(&query_path, "query Test { hello }").unwrap();

        let config = Config::new_test(
            base_dir.clone(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GqlGlobPattern::Single("*.graphql".to_string())),
            ],
        );

        let (service, _) = LspService::new(|client| Backend::new(client, config));
        let backend = service.inner();

        let schema_uri = Url::from_file_path(&schema_path).unwrap();
        let query_uri = Url::from_file_path(&query_path).unwrap();

        backend.documents.insert(
            schema_uri.clone(),
            Arc::new(DocumentState::new_from_thread_local(
                schema_uri.clone(),
                "type Query { hello: String }",
                PositionEncodingKind::UTF16,
            )),
        );
        backend.documents.insert(
            query_uri.clone(),
            Arc::new(DocumentState::new_from_thread_local(
                query_uri.clone(),
                "query Test { hello }",
                PositionEncodingKind::UTF16,
            )),
        );

        let first = backend.get_configured_document_uris();
        assert_eq!(first.len(), 1);
        assert!(first.contains(&query_uri));
        assert!(!first.contains(&schema_uri));

        let second = backend.get_configured_document_uris();
        assert!(Arc::ptr_eq(&first, &second));

        backend.increment_workspace_version();

        let third = backend.get_configured_document_uris();
        assert_eq!(third.len(), 1);
        assert!(third.contains(&query_uri));
        assert!(!third.contains(&schema_uri));
        assert!(!Arc::ptr_eq(&first, &third));

        fs::remove_dir_all(base_dir).unwrap();
    }
}
