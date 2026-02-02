use std::sync::{Arc, RwLock};
use apollo_compiler::Schema;
use dashmap::DashMap;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::SEMANTIC_TOKEN_LEGEND;

pub struct Backend {
    pub client: Client,
    pub documents: DashMap<Url, DocumentState>,
    pub schema: Arc<RwLock<Schema>>,
}

impl Backend {
    pub fn new(client: Client, schema_path: &str) -> Self {
        let schema_text = std::fs::read_to_string(schema_path).unwrap_or_else(|_| "".to_string());
        let schema =
            Schema::parse(&schema_text, schema_path).expect("Failed to parse initial schema");

        Self {
            client,
            documents: DashMap::new(),
            schema: Arc::new(RwLock::new(schema)),
        }
    }

    async fn reload_schema(&self, path: &str) {
        if let Ok(text) = std::fs::read_to_string(path) {
            match Schema::parse(&text, path) {
                Ok(new_schema) => {
                    {
                        let mut lock = self.schema.write().unwrap();
                        *lock = new_schema;
                    }
                    self.client
                        .log_message(MessageType::INFO, "Schema successfully reloaded!")
                        .await;
                }
                Err(e) => {
                    self.client
                        .show_message(MessageType::ERROR, format!("Schema parse error: {}", e))
                        .await;
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

        // Register for schema file changes
        let registration = Registration {
            id: "watch-schema".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/*.graphql".to_string()),
                        kind: Some(WatchKind::all()),
                    }],
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
            let schema = self.schema.read().unwrap();

            if let Some(hover) = doc.get_hover_info(position, &schema) {
                return Ok(Some(hover));
            }

            // If no schema or description hover, check if it's a fragment spread
            if let Some(symbol_name) = doc.get_symbol_at_position(position) {
                for entry in self.documents.iter() {
                    if let Some(info) = entry.value().find_fragment_info(&symbol_name) {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!("```graphql\n{}\n```", info),
                            }),
                            range: None, // We could calculate range here if needed
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
            let schema = self.schema.read().unwrap();

            // Collect all fragments from all documents
            let mut fragments = Vec::new();
            for entry in self.documents.iter() {
                fragments.extend(entry.value().fragments().to_vec());
            }

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

            let diagnostics = {
                let schema = self.schema.read().unwrap();
                let mut fragments = Vec::new();
                for entry in self.documents.iter() {
                    fragments.extend(entry.value().fragments().to_vec());
                }
                doc.get_semantic_diagnostics(&schema, &fragments)
            };

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

        // 2. Search for definition in all documents
        if let Some(name) = symbol_name {
            for entry in self.documents.iter() {
                let doc = entry.value();
                if let Some(location) = doc.find_definition_in_tree(&name) {
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
        self.documents.insert(params.text_document.uri, doc);
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

        let config = self
            .client
            .configuration(vec![ConfigurationItem {
                scope_uri: None,
                section: Some("gqlLsp.schemaPath".to_string()),
            }])
            .await;

        if let Ok(values) = config
            && let Some(path_value) = values.first().and_then(|v| v.as_str())
        {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("New schema path: {}", path_value),
                )
                .await;

            self.reload_schema(path_value).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        for change in params.changes {
            if change.uri.path().ends_with(".graphql") {
                self.client
                    .log_message(MessageType::INFO, "Schema file changed, reloading...")
                    .await;
                self.reload_schema(change.uri.path()).await;
            }
        }
    }
}
