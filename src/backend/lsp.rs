use super::fragment_manager;
use crate::Config;
use crate::config::SchemaSource;
use crate::document::{DocumentLanguage, DocumentState};
use crate::features::completion::FragmentCompletionInfo;
use crate::utils::SEMANTIC_TOKEN_LEGEND;
use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use rayon::prelude::*;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};

/// Client capabilities extracted from initialization
#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
    pub supports_pull_diagnostics: bool,
    pub supports_workspace_folders: bool,
    pub supports_configuration: bool,
    pub supports_progress: bool,
    pub supports_semantic_tokens: bool,
    pub supports_inlay_hints: bool,
}

pub struct Backend {
    pub client: Client,
    pub documents: Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    pub config: Arc<std::sync::RwLock<Config>>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub empty_schema: Arc<Schema>,
    pub valid_empty_schema: Arc<apollo_compiler::validation::Valid<Schema>>,
    pub validated_schemas:
        Arc<DashMap<String, Arc<apollo_compiler::validation::Valid<Schema>>, ahash::RandomState>>,
    // Performance optimizations
    pub fragment_defs: Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    pub fragment_spreads: Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    pub package_roots: Arc<DashMap<Url, Option<std::path::PathBuf>, ahash::RandomState>>,
    pub fragment_dependents: Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub fragment_definitions: Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub workspace_loaded: Arc<AtomicBool>,
    pub open_documents: Arc<dashmap::DashSet<Url, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<AtomicBool>,
    pub gitignore: Arc<ignore::gitignore::Gitignore>,
    /// Persistent type cache per schema (keyed by schema key)
    /// Shared across all codegen runs for the same schema to maximize cache hits
    pub type_caches:
        Arc<DashMap<String, Arc<crate::features::codegen::TypeCache>, ahash::RandomState>>,
    /// Client capabilities for conditional feature enablement
    pub client_capabilities: Arc<std::sync::RwLock<ClientCapabilities>>,
    /// Cached diagnostics for pull-based diagnostics (URI -> (version, diagnostics))
    pub diagnostic_cache: Arc<DashMap<Url, (i32, Vec<Diagnostic>), ahash::RandomState>>,
    /// Throttled codegen runner
    pub codegen_throttle: Option<Arc<super::codegen_throttle::CodegenThrottle>>,
}

impl Backend {
    pub fn new(client: Client, mut config: Config) -> Self {
        // Canonicalize base_dir to ensure consistency on macOS
        if let Ok(canon) = std::fs::canonicalize(&config.base_dir) {
            config.base_dir = canon;
        }

        let schemas = DashMap::with_hasher(ahash::RandomState::default());
        let validated_schemas = DashMap::with_hasher(ahash::RandomState::default());
        let documents = DashMap::with_hasher(ahash::RandomState::default());
        let fragment_definitions: DashMap<String, FnvHashSet<Url>, ahash::RandomState> =
            DashMap::with_hasher(ahash::RandomState::default());

        let empty_schema = Arc::new(
            Schema::parse("type Query { _empty: String }", "empty.graphql")
                .unwrap_or_else(|e| {
                    super::error_logging::log_error_sync(format!(
                        "Failed to parse empty schema (this should never happen): {}",
                        e
                    ));
                    // Fallback to absolutely minimal schema
                    Schema::parse("schema { query: Query } type Query { __typename: String }", "fallback.graphql")
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

        let gitignore = Arc::new(crate::utils::get_gitignore_matcher(&config.base_dir));

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
            workspace_loaded: Arc::new(AtomicBool::new(false)),
            open_documents: Arc::new(dashmap::DashSet::with_hasher(ahash::RandomState::default())),
            workspace_scan_cancelled: Arc::new(AtomicBool::new(false)),
            gitignore,
            type_caches,
            client_capabilities: Arc::new(std::sync::RwLock::new(ClientCapabilities::default())),
            diagnostic_cache: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
            codegen_throttle,
        }
    }

    fn load_schema_source(
        base_dir: &std::path::Path,
        source: &SchemaSource,
    ) -> Option<Arc<Schema>> {
        crate::schema::load_schema_arc(base_dir, source)
    }

    pub fn normalize_uri(&self, uri: Url) -> Url {
        if let Ok(path) = uri.to_file_path()
            && let Ok(canon) = std::fs::canonicalize(&path)
        {
            return Url::from_file_path(canon).unwrap_or(uri);
        }
        uri
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
        let config = self.config.read().unwrap();
        fragment_manager::collect_fragment_metadata(
            &self.fragment_defs,
            &config,
            &self.package_roots,
        )
    }

    pub fn get_fragments_for_doc(&self, doc: &DocumentState) -> Vec<FragmentCompletionInfo> {
        let config = self.config.read().unwrap();
        super::validation::get_fragments_for_doc(
            doc,
            &config,
            &self.fragment_defs,
            &self.package_roots,
        )
    }

    fn get_transitive_fragments(
        &self,
        initial_spreads: Vec<String>,
        package_root: Option<&std::path::PathBuf>,
    ) -> fnv::FnvHashSet<Url> {
        let mut visited_names = fnv::FnvHashSet::default();
        let mut fragment_uris = fnv::FnvHashSet::default();
        let mut to_visit = initial_spreads;

        let all_fragments = self.get_all_fragments_info();

        while let Some(name) = to_visit.pop() {
            if !visited_names.insert(name.clone()) {
                continue;
            }

            // Find this fragment (respecting scoping)
            if let Some(frag) = all_fragments.iter().find(|f| {
                f.name == name && (f.is_public || f.package_root.as_ref() == package_root)
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
    ) -> std::collections::BTreeMap<String, String> {
        let mut requirements = std::collections::BTreeMap::new();
        let mut visited = fnv::FnvHashSet::default();
        self.collect_fragment_requirements_recursive(
            name,
            schema,
            package_root,
            &mut requirements,
            &mut visited,
        );
        requirements
    }

    fn collect_fragment_requirements_recursive(
        &self,
        initial_name: &str,
        schema: &Schema,
        package_root: Option<&std::path::PathBuf>,
        requirements: &mut std::collections::BTreeMap<String, String>,
        visited: &mut fnv::FnvHashSet<String>,
    ) {
        let mut stack = vec![initial_name.to_string()];
        let all_fragments = self.get_all_fragments_info();

        while let Some(name) = stack.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }

            if let Some(frag) = all_fragments.iter().find(|f| {
                f.name == name && (f.is_public || f.package_root.as_ref() == package_root)
            }) && let Some(doc) = self.documents.get(&frag.uri).map(|r| r.value().clone())
            {
                // Get variables from this fragment
                let local_vars = doc.get_fragment_variable_types(&name, schema);
                for (var, ty) in local_vars {
                    requirements.insert(var, ty);
                }

                // Get nested fragments
                if let Some(def) = doc.fragments().iter().find(|f| f.name == name) {
                    for nested in &def.used_fragments {
                        stack.push(nested.clone());
                    }
                }
            }
        }
    }

    pub fn get_used_fragments(&self) -> fnv::FnvHashSet<String> {
        super::validation::get_used_fragments(&self.fragment_spreads)
    }

    async fn with_tracing<T, Fut>(&self, name: &str, fut: Fut) -> Result<Option<T>>
    where
        Fut: std::future::Future<Output = Result<Option<T>>>,
    {
        let start = std::time::Instant::now();
        
        // Get timeout duration
        let timeout_ms = {
            let config = self.config.read().unwrap();
            config.get_timeouts().lsp_request_ms
        };
        
        // Apply timeout
        let res = match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            fut
        ).await {
            Ok(result) => result,
            Err(_) => {
                let elapsed = start.elapsed();
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "LSP Request '{}' exceeded timeout of {}ms (took {}ms) - returning empty response",
                            name,
                            timeout_ms,
                            elapsed.as_millis()
                        ),
                    )
                    .await;
                // Return Ok(None) for timed out requests
                Ok(None)
            }
        };

        // Extract tracing config before await
        let should_log = {
            let config = self.config.read().unwrap();
            config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
        };

        if let Some((enabled, threshold_ms)) = should_log
            && enabled {
                let elapsed = start.elapsed();
                if elapsed.as_millis() >= threshold_ms as u128 {
                    self.client
                        .log_message(
                            MessageType::INFO,
                            format!("LSP Request '{}' took {}ms", name, elapsed.as_millis()),
                        )
                        .await;
                }
            }
        res
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

        // Validate documents affected by reloaded schemas
        for key in reloaded_keys {
            let affected =
                super::schema_management::get_uris_affected_by_schema(&key, &config, || {
                    self.documents.iter().map(|e| e.key().clone()).collect()
                });
            self.validate_uris(affected).await;
        }
    }

    async fn clear_cache(&self) {
        let config = self.config.read().unwrap().clone();
        super::schema_management::clear_cache(
            &config,
            &self.schemas,
            &self.validated_schemas,
            &self.client,
        )
        .await;

        // Re-validate all open documents
        self.validate_all_documents().await;
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

        // Only clear non-open documents to preserve user's open files
        let open_uris: Vec<_> = self
            .open_documents
            .iter()
            .map(|r| r.key().clone())
            .collect();
        self.documents.retain(|uri, _| open_uris.contains(uri));

        self.fragment_defs.clear();
        self.fragment_spreads.clear();
        self.fragment_dependents.clear();
        self.fragment_definitions.clear();
        self.package_roots.clear();
        self.type_caches.clear();

        // Re-register file watchers with new config
        {
            let config = self.config.read().unwrap();
            super::file_watchers::register_file_watchers(self.client.clone(), &config);
        }

        // Reload schemas from new config
        let config = self.config.read().unwrap().clone();
        for project in &config.projects {
            let key = project.schema.as_key();
            if !self.schemas.contains_key(&key)
                && let Some(schema) = Self::load_schema_source(&config.base_dir, &project.schema) {
                    if let Ok(valid) = (*schema).clone().validate() {
                        self.validated_schemas.insert(key.clone(), Arc::new(valid));
                    }
                    self.schemas.insert(key, schema);
                }
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
            workspace_loaded: self.workspace_loaded.clone(),
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            supports_progress,
        });

        self.client
            .log_message(MessageType::INFO, "Configuration reloaded successfully")
            .await;
    }

    fn update_dependency_indices(
        &self,
        uri: &Url,
        old_spreads: Option<Vec<String>>,
        new_spreads: Vec<String>,
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
        old_fragments: Option<Vec<String>>,
        new_fragments: Vec<String>,
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
            supports_progress,
        };
        super::validation::validate_all_documents(params, use_push, Some(&self.diagnostic_cache))
            .await;
    }

    fn get_affected_uris(
        &self,
        initial_uri: Url,
        affected_fragment_names: FnvHashSet<String>,
        affected_spread_names: FnvHashSet<String>,
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
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Extract and store client capabilities
        let client_caps = &params.capabilities;

        let mut caps = ClientCapabilities::default();

        // Check for pull diagnostics support (LSP 3.17+)
        if let Some(text_document) = &client_caps.text_document {
            caps.supports_pull_diagnostics = text_document.diagnostic.is_some();
        }

        // Check for workspace folder support
        if let Some(workspace) = &client_caps.workspace {
            caps.supports_workspace_folders = workspace.workspace_folders.unwrap_or(false);
            caps.supports_configuration = workspace.configuration.unwrap_or(false);
        }

        // Check for progress support
        if let Some(window) = &client_caps.window {
            caps.supports_progress = window.work_done_progress.unwrap_or(false);
        }

        // Check for semantic tokens support
        if let Some(text_document) = &client_caps.text_document {
            caps.supports_semantic_tokens = text_document.semantic_tokens.is_some();
        }

        // Check for inlay hints support (for future implementation)
        if let Some(text_document) = &client_caps.text_document {
            caps.supports_inlay_hints = text_document.inlay_hint.is_some();
        }

        // Store capabilities
        if let Ok(mut stored_caps) = self.client_capabilities.write() {
            *stored_caps = caps.clone();
        }

        // Log detected capabilities
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "Client capabilities: pull_diagnostics={}, workspace_folders={}, progress={}, semantic_tokens={}, inlay_hints={}",
                    caps.supports_pull_diagnostics,
                    caps.supports_workspace_folders,
                    caps.supports_progress,
                    caps.supports_semantic_tokens,
                    caps.supports_inlay_hints,
                ),
            )
            .await;

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: None,
                    file_operations: None,
                }),
                semantic_tokens_provider: if caps.supports_semantic_tokens {
                    Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions {
                                work_done_progress: None,
                            },
                            legend: SemanticTokensLegend {
                                token_types: SEMANTIC_TOKEN_LEGEND.to_vec(),
                                token_modifiers: vec![],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ))
                } else {
                    None
                },
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "graphql.runCodegen".to_string(),
                        "graphql.clearCache".to_string(),
                    ],
                    ..Default::default()
                }),
                diagnostic_provider: if caps.supports_pull_diagnostics {
                    Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                        identifier: Some("graphql-rust".to_string()),
                        inter_file_dependencies: true,
                        workspace_diagnostics: true,
                        work_done_progress_options: Default::default(),
                    }))
                } else {
                    None
                },
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
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
            workspace_loaded: self.workspace_loaded.clone(),
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.clone(),
            supports_progress,
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
                        let is_same_package = other_doc.package_root == doc.package_root;
                        let is_public_fragment = other_doc
                            .fragments()
                            .iter()
                            .any(|f| f.name == symbol_name && f.is_public);

                        if (is_same_package || is_public_fragment)
                            && let Some(info) = other_doc.find_fragment_info(&symbol_name)
                        {
                            let mut value = format!("```graphql\n{}\n```", info);

                            let requirements = self.get_fragment_requirements(
                                &symbol_name,
                                &schema,
                                doc.package_root.as_ref(),
                            );
                            if !requirements.is_empty() {
                                value.push_str("\n\n**Requires Variables:**\n");
                                for (var, ty) in requirements {
                                    value.push_str(&format!("- `${}`: `{}`\n", var, ty));
                                }
                            }

                            if let Some(desc) = other_doc.find_description(&symbol_name) {
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
                let mut fragments = self.get_fragments_for_doc(&doc);

                for f in &mut fragments {
                    f.requirements =
                        self.get_fragment_requirements(&f.name, &schema, doc.package_root.as_ref());
                }

                let items = doc.get_completion_items(position, &schema, fragments);
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

        let mut affected_fragment_names = FnvHashSet::default();
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
        }

        // Update performance indices
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

        let mut affected_spread_names = FnvHashSet::default();
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
            let uri = self.normalize_uri(
                params
                    .text_document_position_params
                    .text_document
                    .uri
                    .clone(),
            );
            let position = params.text_document_position_params.position;

            if let Some(doc_arc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = self.get_schema_for_doc(&uri);

                let symbol_name = doc_arc.get_symbol_at_position(position);

                if let Some(ref name) = symbol_name
                    && name.starts_with('$')
                    && let Some(location) = doc_arc.find_variable_definition(name, position)
                {
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }

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

                if let Some(loc) = doc_arc.get_field_definition_location(
                    position,
                    &schema,
                    &self.documents,
                    &preferred_uris,
                    &self.fragment_definitions,
                ) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
                }

                if let Some(name) = symbol_name {
                    // Targeted lookup using the index
                    if let Some(uris) = self.fragment_definitions.get(&name) {
                        for other_uri in uris.iter() {
                            if let Some(other_doc) =
                                self.documents.get(other_uri).map(|r| r.value().clone())
                            {
                                let is_same_package =
                                    other_doc.package_root == doc_arc.package_root;
                                let is_public_fragment = other_doc
                                    .fragments()
                                    .iter()
                                    .any(|f| f.name == name && f.is_public);

                                if (is_same_package || is_public_fragment)
                                    && let Some(location) = other_doc.find_definition_in_tree(&name)
                                {
                                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                                }
                            }
                        }
                    }

                    // Fallback to full scan if not in index
                    if !self.fragment_definitions.contains_key(&name) {
                        let doc_arcs: Vec<Arc<DocumentState>> =
                            self.documents.iter().map(|e| e.value().clone()).collect();

                        let result = doc_arcs.par_iter().find_map_any(|other_doc| {
                            let is_same_package = other_doc.package_root == doc_arc.package_root;
                            let is_public_fragment = other_doc
                                .fragments()
                                .iter()
                                .any(|f| f.name == name && f.is_public);

                            if (is_same_package || is_public_fragment)
                                && let Some(location) = other_doc.find_definition_in_tree(&name)
                            {
                                Some(location)
                            } else {
                                None
                            }
                        });

                        if let Some(location) = result {
                            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                        }
                    }
                }
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
                let symbol_name = doc.get_symbol_at_position(position);

                if let Some(name) = symbol_name
                    && name.starts_with('$') {
                        // Get highlights in the current document only
                        return Ok(doc.get_document_highlights(position));
                    }
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
                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            uri.clone(),
                            vec![TextEdit {
                                range: diagnostic.range,
                                new_text: String::new(),
                            }],
                        );

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Remove @type_only directive".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));
                    } else if code == "missing_field" {
                        if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone()) {
                            let field_actions = doc.get_missing_field_actions(&diagnostic);
                            for action in field_actions {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
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
        let _res = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            async move {
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
                    if result.should_run_codegen {
                        if let Some(throttle) = &self.codegen_throttle {
                            throttle.request_codegen();
                        }
                    }
                }
            }
        }
        ).await;
        
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
            && enabled {
                let elapsed = start.elapsed();
                if elapsed.as_millis() >= threshold_ms as u128 {
                    self.client
                        .log_message(
                            MessageType::INFO,
                            format!("LSP Request 'did_change_watched_files' took {}ms", elapsed.as_millis()),
                        )
                        .await;
                }
            }
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
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
                        && prev_version == *cached_version && prev_version == doc_version {
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
        }
        ).await {
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
            && enabled {
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
        }
        ).await {
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
            && enabled {
                let elapsed = start.elapsed();
                if elapsed.as_millis() >= threshold_ms as u128 {
                    self.client
                        .log_message(
                            MessageType::INFO,
                            format!("LSP Request 'workspace_diagnostic' took {}ms", elapsed.as_millis()),
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
    use crate::config::{Config, GlobPattern, ProjectConfig, SchemaSource};
    use tokio::time::{Duration, timeout};
    use tower_lsp::LspService;

    #[tokio::test]
    async fn test_validate_all_documents_performance() {
        let config = Config {
            base_dir: std::env::current_dir().unwrap(),
            projects: vec![ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: None,
            }],
            output_dir: None,
            schema_types: None,
            scalars: None,
            ignore_deprecations: None,
            generate_ast_for_fragments: None,
            tracing: None,
            timeouts: None,
            watch_all_files: None,
            lsp_automatic_codegen: None,
            lsp_codegen_throttle_ms: None,
            enable_schema_cache: None,
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
            output_dir: None,
            schema_types: None,
            scalars: None,
            ignore_deprecations: None,
            generate_ast_for_fragments: None,
            tracing: None,
            timeouts: None,
            watch_all_files: None,
            lsp_automatic_codegen: None,
            lsp_codegen_throttle_ms: None,
            enable_schema_cache: None,
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
