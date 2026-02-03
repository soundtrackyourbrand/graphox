use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use std::sync::Arc;
use tower_lsp::{jsonrpc::Result, lsp_types::*, Client, LanguageServer};
use crate::config::SchemaSource;
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::SEMANTIC_TOKEN_LEGEND;
use crate::Config;

pub struct Backend {
    pub client: Client,
    pub documents: DashMap<Url, DocumentState>,
    pub config: Config,
    pub schemas: DashMap<String, Arc<Schema>>,
    pub empty_schema: Arc<Schema>,
}

impl Backend {
    pub fn new(client: Client, config: Config) -> Self {
        let schemas = DashMap::new();
        let empty_schema = Arc::new(
            Schema::parse("type Query { _empty: String }", "empty.graphql").unwrap(),
        );

        // Load project schemas from config
        for project in &config.projects {
            let key = project.schema.as_key();
            if !schemas.contains_key(&key)
                && let Some(schema) = Self::load_schema_source(&config.base_dir, &project.schema)
            {
                schemas.insert(key, schema);
            }
        }

        Self {
            client,
            documents: DashMap::new(),
            config,
            schemas,
            empty_schema,
        }
    }

    fn load_schema_source(base_dir: &std::path::Path, source: &SchemaSource) -> Option<Arc<Schema>> {
        let mut combined_text = String::new();
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
        Schema::parse(&combined_text, source.as_key()).ok().map(Arc::new)
    }

    fn get_schema_for_doc(&self, uri: &Url) -> Arc<Schema> {
        if let Ok(path) = uri.to_file_path()
            && let Some(schema_path) = self.config.get_schema_for_path(&path)
            && let Some(schema) = self.schemas.get(&schema_path)
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
        for project in &self.config.projects {
            if project.schema.files().iter().any(|f| {
                let abs = self.config.base_dir.join(f);
                abs.to_string_lossy() == changed_path
                    || abs.canonicalize().ok().map(|p| p.to_string_lossy().to_string())
                        == Some(changed_path.to_string())
            }) {
                sources_to_reload.push(project.schema.clone());
            }
        }
        if let Some(schema_types) = &self.config.schema_types {
            for st in schema_types {
                if st.schema.files().iter().any(|f| {
                    let abs = self.config.base_dir.join(f);
                    abs.to_string_lossy() == changed_path
                        || abs.canonicalize().ok().map(|p| p.to_string_lossy().to_string())
                            == Some(changed_path.to_string())
                }) {
                    sources_to_reload.push(st.schema.clone());
                }
            }
        }

        for source in sources_to_reload {
            let key = source.as_key();
            let new_schema = Self::load_schema_source(&self.config.base_dir, &source);

            if let Some(new_schema) = new_schema {
                self.schemas.insert(key.clone(), new_schema.clone());
                self.client
                    .log_message(MessageType::INFO, format!("Schema set {} successfully reloaded!", key))
                    .await;

                let mut to_publish = Vec::new();
                for entry in self.documents.iter() {
                    let uri = entry.key();
                    let doc = entry.value();
                    let doc_schema = self.get_schema_for_doc(uri);
                    if let Ok(doc_path) = uri.to_file_path() {
                        if self
                            .config
                            .get_schema_for_path(&doc_path)
                            .is_some_and(|p| p.as_str() == key.as_str())
                        {
                            let fragments = self.get_fragments_for_doc(doc);
                            let diagnostics = doc.get_semantic_diagnostics(
                                &doc_schema,
                                &fragments,
                                Some(&self.config),
                                false,
                            );
                            to_publish.push((uri.clone(), diagnostics));
                        }
                    }
                }
                for (uri, diagnostics) in to_publish {
                    self.client.publish_diagnostics(uri, diagnostics, None).await;
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
        let mut schema_files = FnvHashSet::default();
        for project in &self.config.projects {
            for file in project.schema.files() {
                schema_files.insert(file);
            }
        }
        if let Some(schema_types) = &self.config.schema_types {
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
        }

        let mut to_publish = Vec::new();
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let diagnostics = doc.get_semantic_diagnostics(&schema, &fragments, Some(&self.config), false);
            drop(doc);
            to_publish.push((uri.clone(), diagnostics));
        }

        // Also revalidate other documents because fragments might have changed
        for entry in self.documents.iter() {
            let other_uri = entry.key();
            if other_uri == &uri {
                continue;
            }
            let other_doc = entry.value();
            let schema = self.get_schema_for_doc(other_uri);
            let fragments = self.get_fragments_for_doc(other_doc);
            let diagnostics =
                other_doc.get_semantic_diagnostics(&schema, &fragments, Some(&self.config), false);
            to_publish.push((other_uri.clone(), diagnostics));
        }

        for (u, d) in to_publish {
            self.client.publish_diagnostics(u, d, None).await;
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let symbol_name = if let Some(doc) = self.documents.get(&uri) {
            doc.get_symbol_at_position(position)
        } else {
            None
        };

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

        let mut to_publish = Vec::new();
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let diagnostics = doc.get_semantic_diagnostics(&schema, &fragments, Some(&self.config), false);
            drop(doc);
            to_publish.push((uri.clone(), diagnostics));
        }

        // Also revalidate other documents because a new fragment might have been added
        for entry in self.documents.iter() {
            let other_uri = entry.key();
            if other_uri == &uri {
                continue;
            }
            let other_doc = entry.value();
            let schema = self.get_schema_for_doc(other_uri);
            let fragments = self.get_fragments_for_doc(other_doc);
            let diagnostics =
                other_doc.get_semantic_diagnostics(&schema, &fragments, Some(&self.config), false);
            to_publish.push((other_uri.clone(), diagnostics));
        }

        for (u, d) in to_publish {
            self.client.publish_diagnostics(u, d, None).await;
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
