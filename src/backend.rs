use apollo_compiler::Schema;
use dashmap::DashMap;
use std::sync::Arc;
use tower_lsp::{jsonrpc::Result, lsp_types::*, Client, LanguageServer};
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::SEMANTIC_TOKEN_LEGEND;
use crate::Config;

pub struct Backend {
    pub client: Client,
    pub documents: DashMap<Url, DocumentState>,
    pub config: Option<Config>,
    pub schemas: DashMap<String, Arc<Schema>>,
    pub empty_schema: Arc<Schema>,
    pub default_schema_path: Option<String>,
}

impl Backend {
    pub fn new(client: Client, config: Option<Config>, default_schema_path: &str) -> Self {
        let schemas = DashMap::new();
        let empty_schema = Arc::new(Schema::parse("type Query { _empty: String }", "empty.graphql").unwrap());

        if let Some(cfg) = &config {
            // Load project schemas from config
            for project in &cfg.projects {
                let key = project.schema.as_key();
                if !schemas.contains_key(&key) {
                    if let Some(schema) = Self::load_schema_source(&cfg.base_dir, &project.schema) {
                        schemas.insert(key, schema);
                    }
                }
            }
        }

        // Always try to load the CLI schema too, just in case it's used as fallback when no config is present
        if let Ok(text) = std::fs::read_to_string(default_schema_path)
            && let Ok(schema) = Schema::parse(&text, default_schema_path)
        {
            schemas.insert(default_schema_path.to_string(), Arc::new(schema));
        }

        Self {
            client,
            documents: DashMap::new(),
            config,
            schemas,
            empty_schema,
            default_schema_path: Some(default_schema_path.to_string()),
        }
    }

    fn load_schema_source(base_dir: &std::path::Path, source: &crate::config::SchemaSource) -> Option<Arc<Schema>> {
        let mut combined_text = String::new();
        let key = source.as_key();
        for file in source.files() {
            let path = base_dir.join(file);
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    combined_text.push_str(&text);
                    combined_text.push('\n');
                }
                Err(_) => return None,
            }
        }
        Schema::parse(&combined_text, &key).ok().map(Arc::new)
    }

    fn get_schema_for_doc(&self, uri: &Url) -> Arc<Schema> {
        if let Some(config) = &self.config {
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_path) = config.get_schema_for_path(&path)
                && let Some(schema) = self.schemas.get(&schema_path)
            {
                return schema.value().clone();
            }
            // If we have a config but no match, user said "assume empty schema"
            return self.empty_schema.clone();
        }

        // No config present at all, fallback to default schema (preserves CLI/test behavior)
        if let Some(default_path) = &self.default_schema_path
            && let Some(schema) = self.schemas.get(default_path)
        {
            return schema.value().clone();
        }

        self.empty_schema.clone()
    }

    fn get_fragments_for_doc(&self, doc: &DocumentState) -> Vec<String> {
        let mut fragments = Vec::new();
        for entry in self.documents.iter() {
            let other_doc = entry.value();
            let is_same_package = other_doc.package_root == doc.package_root;
            for frag in other_doc.fragments() {
                if is_same_package || frag.is_public {
                    fragments.push(frag.name.clone());
                }
            }
        }
        fragments
    }

    async fn reload_schema(&self, changed_path: &str) {
        let mut sources_to_reload = Vec::new();
        if let Some(cfg) = &self.config {
            for project in &cfg.projects {
                if project.schema.files().iter().any(|f| {
                    let abs = cfg.base_dir.join(f);
                    abs.to_string_lossy() == changed_path || abs.canonicalize().ok().map(|p| p.to_string_lossy().to_string()) == Some(changed_path.to_string())
                }) {
                    sources_to_reload.push(project.schema.clone());
                }
            }
            if let Some(schema_types) = &cfg.schema_types {
                for st in schema_types {
                    if st.schema.files().iter().any(|f| {
                        let abs = cfg.base_dir.join(f);
                        abs.to_string_lossy() == changed_path || abs.canonicalize().ok().map(|p| p.to_string_lossy().to_string()) == Some(changed_path.to_string())
                    }) {
                        sources_to_reload.push(st.schema.clone());
                    }
                }
            }
        }

        if sources_to_reload.is_empty() && self.default_schema_path.as_ref().is_some_and(|p| p == changed_path) {
            sources_to_reload.push(crate::config::SchemaSource::Single(changed_path.to_string()));
        }

        for source in sources_to_reload {
            let key = source.as_key();
            let new_schema = if let Some(cfg) = &self.config {
                Self::load_schema_source(&cfg.base_dir, &source)
            } else {
                std::fs::read_to_string(changed_path).ok().and_then(|text| {
                    Schema::parse(&text, changed_path).ok().map(Arc::new)
                })
            };

            if let Some(new_schema) = new_schema {
                self.schemas.insert(key.clone(), new_schema.clone());
                self.client
                    .log_message(MessageType::INFO, format!("Schema set {} successfully reloaded!", key))
                    .await;

                // Re-validate all documents that use this schema
                for entry in self.documents.iter() {
                    let uri = entry.key();
                    let doc = entry.value();

                    let doc_schema = self.get_schema_for_doc(uri);

                    if let Ok(doc_path) = uri.to_file_path() {
                        let matches = if let Some(config) = &self.config {
                            config.get_schema_for_path(&doc_path).is_some_and(|p| p == key)
                        } else {
                            self.default_schema_path.as_ref().is_some_and(|p| p == &key)
                        };

                        if matches {
                            let fragments = self.get_fragments_for_doc(doc);
                            let diagnostics = doc.get_semantic_diagnostics(&doc_schema, &fragments);
                            self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                        }
                    }
                }
            }
        }
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
                definition_provider: Some(OneOf::Left(true)),
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
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "LSP Started!")
            .await;

        let mut watchers = Vec::new();
        if let Some(cfg) = &self.config {
            let mut schema_files = std::collections::HashSet::new();
            for project in &cfg.projects {
                for file in project.schema.files() {
                    schema_files.insert(file);
                }
            }
            if let Some(schema_types) = &cfg.schema_types {
                for st in schema_types {
                    for file in st.schema.files() {
                        schema_files.insert(file);
                    }
                }
            }

            for file in schema_files {
                watchers.push(FileSystemWatcher {
                    glob_pattern: GlobPattern::String(file),
                    kind: Some(WatchKind::all()),
                });
            }
        } else if let Some(default_path) = &self.default_schema_path {
            watchers.push(FileSystemWatcher {
                glob_pattern: GlobPattern::String(default_path.clone()),
                kind: Some(WatchKind::all()),
            });
        } else {
            watchers.push(FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.graphql".to_string()),
                kind: Some(WatchKind::all()),
            });
        }

        // Register for schema file changes
        let registration = Registration {
            id: "watch-schema".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers,
                })
                .unwrap(),
            ),
        };
        if let Err(e) = self.client.register_capability(vec![registration]).await {
            self.client
                .log_message(
                    MessageType::ERROR,
                    format!("Failed to register schema watcher: {}", e),
                )
                .await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(uri) {
            let schema = self.get_schema_for_doc(uri);
            if let Some(hover) = doc.get_hover_info(position, &schema) {
                return Ok(Some(hover));
            }

            // If no schema or description hover, check if it's a fragment spread
            if let Some(symbol_name) = doc.get_symbol_at_position(position) {
                for entry in self.documents.iter() {
                    let other_doc = entry.value();
                    let is_same_package = other_doc.package_root == doc.package_root;
                    let is_public_fragment = other_doc
                        .fragments()
                        .iter()
                        .any(|f| f.name == symbol_name && f.is_public);

                    if (is_same_package || is_public_fragment)
                        && let Some(info) = other_doc.find_fragment_info(&symbol_name)
                    {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!("```graphql\n{}\n```", info),
                            }),
                            range: None,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Some(doc) = self.documents.get(uri) {
            let schema = self.get_schema_for_doc(uri);
            // Collect fragments from the same package
            let fragments = self.get_fragments_for_doc(&doc);

            let items = doc.get_completion_items(position, &schema, fragments);
            return Ok(Some(CompletionResponse::Array(items)));
        }

        Ok(None)
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        if let Some(mut doc) = self.documents.get_mut(&uri) {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&doc.language.get_parser_language())
                .unwrap();

            for change in params.content_changes {
                doc.apply_change(&change, &mut parser);
            }

            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let diagnostics = doc.get_semantic_diagnostics(&schema, &fragments);
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // 1. Find the symbol name at the cursor
        let symbol_name = if let Some(doc) = self.documents.get(&uri) {
            doc.get_symbol_at_position(position)
        } else {
            None
        };

        // 2. Search for definition in documents within the same package or public fragments
        if let Some(name) = symbol_name
            && let Some(doc) = self.documents.get(&uri)
        {
            for entry in self.documents.iter() {
                let other_doc = entry.value();
                let is_same_package = other_doc.package_root == doc.package_root;
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

        Ok(None)
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let language = DocumentLanguage::from_uri(&params.text_document.uri);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.get_parser_language())
            .unwrap();

        let doc = DocumentState::new(
            params.text_document.uri.clone(),
            &params.text_document.text,
            parser,
        );
        let uri = params.text_document.uri;
        self.documents.insert(uri.clone(), doc);

        // Initial validation
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let diagnostics = doc.get_semantic_diagnostics(&schema, &fragments);
            self.client.publish_diagnostics(uri, diagnostics, None).await;
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        if let Some(doc) = self.documents.get(&params.text_document.uri) {
            let symbols = doc.get_symbols();
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }

        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        if let Some(doc) = self.documents.get(&params.text_document.uri) {
            let tokens = doc.get_semantic_tokens();
            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })));
        }

        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.client
            .log_message(MessageType::INFO, "Configuration changed!")
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        for change in params.changes {
            if change.uri.path().ends_with(".graphql") {
                self.reload_schema(change.uri.path()).await;
            }
        }
    }
}
