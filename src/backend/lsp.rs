use crate::Config;
use crate::config::SchemaSource;
use crate::document::{DocumentLanguage, DocumentState};
use crate::features::completion::FragmentCompletionInfo;
use crate::utils::SEMANTIC_TOKEN_LEGEND;
use super::fragment_manager;
use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use rayon::prelude::*;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};

pub struct Backend {
    pub client: Client,
    pub documents: Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    pub config: Config,
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
    pub type_caches: Arc<DashMap<String, Arc<crate::features::codegen::TypeCache>, ahash::RandomState>>,
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

        let empty_schema =
            Arc::new(Schema::parse("type Query { _empty: String }", "empty.graphql").unwrap());
        let valid_empty_schema = Arc::new((*empty_schema).clone().validate().unwrap());

        // Load project schemas from config
        for project in &config.projects {
            let key = project.schema.as_key();
            if !schemas.contains_key(&key)
                && let Some(schema) = Self::load_schema_source(&config.base_dir, &project.schema)
            {
                if let Ok(valid) = (*schema).clone().validate() {
                    validated_schemas.insert(key.clone(), Arc::new(valid));
                }
                schemas.insert(key, schema);
            }
        }

        let gitignore = Arc::new(crate::utils::get_gitignore_matcher(&config.base_dir));

        Self {
            client,
            documents: Arc::new(documents),
            config,

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
            type_caches: Arc::new(DashMap::with_hasher(ahash::RandomState::default())),
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
        super::validation::get_schema_for_doc(
            uri,
            &self.config,
            &self.validated_schemas,
            &self.valid_empty_schema,
        )
    }

    pub fn get_all_fragments_info(&self) -> Vec<FragmentCompletionInfo> {
        fragment_manager::collect_fragment_metadata(
            &self.fragment_defs,
            &self.config,
            &self.package_roots,
        )
    }

    pub fn get_fragments_for_doc(&self, doc: &DocumentState) -> Vec<FragmentCompletionInfo> {
        super::validation::get_fragments_for_doc(
            doc,
            &self.config,
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
            })
                && let Some(doc) = self.documents.get(&frag.uri).map(|r| r.value().clone()) {
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

    async fn with_tracing<T, Fut>(&self, name: &str, fut: Fut) -> Fut::Output
    where
        Fut: std::future::Future<Output = T>,
    {
        let start = std::time::Instant::now();
        let res = fut.await;
        if let Some(tracing) = &self.config.tracing
            && tracing.enabled
        {
            let elapsed = start.elapsed();
            if elapsed.as_millis() >= tracing.threshold_ms as u128 {
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
        let reloaded_keys = super::schema_management::reload_schema(
            changed_path,
            &self.config,
            &self.schemas,
            &self.validated_schemas,
            &self.client,
        )
        .await;

        // Validate documents affected by reloaded schemas
        for key in reloaded_keys {
            let affected = super::schema_management::get_uris_affected_by_schema(
                &key,
                &self.config,
                || self.documents.iter().map(|e| e.key().clone()).collect(),
            );
            self.validate_uris(affected).await;
        }
    }

    async fn clear_cache(&self) {
        super::schema_management::clear_cache(
            &self.config,
            &self.schemas,
            &self.validated_schemas,
            &self.client,
        )
        .await;

        // Re-validate all open documents
        self.validate_all_documents().await;
    }

    pub async fn run_codegen(&self) {
        super::codegen_runner::run_codegen(self.client.clone(), self.config.clone(), self.type_caches.clone()).await;
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
        let params = super::validation::ValidationParams {
            client: &self.client,
            documents: &self.documents,
            config: &self.config,
            fragment_defs: &self.fragment_defs,
            fragment_spreads: &self.fragment_spreads,
            package_roots: &self.package_roots,
            validated_schemas: &self.validated_schemas,
            valid_empty_schema: &self.valid_empty_schema,
            workspace_loaded: &self.workspace_loaded,
            open_documents: &self.open_documents,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
        };
        super::validation::validate_uris(params, uris).await;
    }

    pub async fn validate_all_documents(&self) {
        let params = super::validation::ValidationParams {
            client: &self.client,
            documents: &self.documents,
            config: &self.config,
            fragment_defs: &self.fragment_defs,
            fragment_spreads: &self.fragment_spreads,
            package_roots: &self.package_roots,
            validated_schemas: &self.validated_schemas,
            valid_empty_schema: &self.valid_empty_schema,
            workspace_loaded: &self.workspace_loaded,
            open_documents: &self.open_documents,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
        };
        super::validation::validate_all_documents(params).await;
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
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
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
                    ),
                ),
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
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "LSP Started!")
            .await;

        if let Some(tracing) = &self.config.tracing
            && tracing.enabled
        {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!(
                        "Performance tracing enabled (threshold: {}ms)",
                        tracing.threshold_ms
                    ),
                )
                .await;
        }

        // Spawn workspace scan in background to avoid hanging the LSP
        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config: self.config.clone(),
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
        });

        // Register file watchers
        super::file_watchers::register_file_watchers(self.client.clone(), &self.config);
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

                            if !is_same_package && let Ok(other_p) = other_doc.uri.to_file_path()
                                && let Some(proj) = self.config.get_project_for_path(&other_p)
                                    && let Some(import) = &proj.import {
                                        value.push_str("\n\n---\n");
                                        value.push_str(&format!("Import: `{}`", import));
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
        let uri = self.normalize_uri(params.text_document_position_params.text_document.uri);
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
            let schema = self.get_schema_for_doc(&uri);
            return Ok(doc.get_signature_help(position, &schema));
        }

        Ok(None)
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
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
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
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
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
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

        if self.config.lsp_automatic_codegen() {
            let client = self.client.clone();
            let config = self.config.clone();
            let type_caches = self.type_caches.clone();
            tokio::spawn(async move {
                super::codegen_runner::run_codegen(client, config, type_caches).await;
            });
        }
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri);
        self.open_documents.remove(&uri);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri.clone());

        // Process document changes and update indices
        let change_params = super::document_changes::DocumentChangeParams {
            documents: &self.documents,
            fragment_defs: &self.fragment_defs,
            fragment_spreads: &self.fragment_spreads,
            package_roots: &self.package_roots,
            fragment_dependents: &self.fragment_dependents,
            fragment_definitions: &self.fragment_definitions,
        };

        if let Some(result) = super::document_changes::process_document_change(&uri, params.content_changes, &change_params) {
            // Validate affected documents
            self.validate_uris(result.uris_to_validate).await;

            // Run codegen if enabled
            if self.config.lsp_automatic_codegen() {
                let client = self.client.clone();
                let config = self.config.clone();
                let type_caches = self.type_caches.clone();
                tokio::spawn(async move {
                    super::codegen_runner::run_codegen(client, config, type_caches).await;
                });
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
                if let Ok(path) = uri.to_file_path()
                    && let Some(project) = self.config.get_project_for_path(&path) {
                        for schema_file in project.schema.files() {
                            let schema_path = self.config.base_dir.join(schema_file);
                            if let Ok(schema_uri) = Url::from_file_path(schema_path) {
                                preferred_uris.push(schema_uri);
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
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = self.normalize_uri(params.text_document.uri);
        if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
            let symbols = doc.get_symbols();
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }
        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = self.normalize_uri(params.text_document.uri);
        if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
            let tokens = doc.get_semantic_tokens();
            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })));
        }
        Ok(None)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
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
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
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
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.client
            .log_message(MessageType::INFO, "Configuration changed!")
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.with_tracing("did_change_watched_files", async move {
            for change in params.changes {
                let change_params = super::file_change_handler::FileChangeParams {
                    client: &self.client,
                    config: &self.config,
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
                    if result.should_reload_schema
                        && let Some(schema_path) = result.schema_path {
                            self.reload_schema(&schema_path).await;
                        }

                    if !result.uris_to_validate.is_empty() {
                        self.validate_uris(result.uris_to_validate).await;
                    }

                    if result.should_run_codegen {
                        let client = self.client.clone();
                        let config = self.config.clone();
                        let type_caches = self.type_caches.clone();
                        tokio::spawn(async move {
                            super::codegen_runner::run_codegen(client, config, type_caches).await;
                        });
                    }
                }
            }
        })
        .await
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
            }],
            output_dir: None,
            schema_types: None,
            scalars: None,
            ignore_deprecations: None,
            generate_ast_for_fragments: None,
            tracing: None,
            watch_all_files: None,
            lsp_automatic_codegen: None,
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
            watch_all_files: None,
            lsp_automatic_codegen: None,
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
