use crate::Config;
use crate::config::SchemaSource;
use crate::document::{DocumentLanguage, DocumentState};
use crate::features::completion::FragmentCompletionInfo;
use crate::utils::SEMANTIC_TOKEN_LEGEND;
use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use std::sync::Arc;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};

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
        let empty_schema =
            Arc::new(Schema::parse("type Query { _empty: String }", "empty.graphql").unwrap());

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

    fn load_schema_source(
        base_dir: &std::path::Path,
        source: &SchemaSource,
    ) -> Option<Arc<Schema>> {
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
        Schema::parse(&combined_text, source.as_key())
            .ok()
            .map(Arc::new)
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

    fn get_fragments_for_doc(&self, doc: &DocumentState) -> Vec<FragmentCompletionInfo> {
        let mut fragments = Vec::new();
        for entry in self.documents.iter() {
            let other_doc = entry.value();
            let is_same_package = other_doc.package_root == doc.package_root;
            
            let other_path = if let Ok(p) = other_doc.uri.to_file_path() {
                Some(p)
            } else {
                None
            };
            
            let import_path = other_path.as_ref().and_then(|p| {
                self.config.get_project_for_path(p).and_then(|proj| proj.import.clone())
            });

            for frag in other_doc.fragments() {
                if is_same_package || frag.is_public {
                    fragments.push(FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: other_doc.find_description(&frag.name),
                        import_path: if is_same_package { None } else { import_path.clone() },
                    });
                }
            }
        }
        fragments
    }

    fn get_used_fragments(&self) -> fnv::FnvHashSet<String> {
        let mut used = fnv::FnvHashSet::default();
        for entry in self.documents.iter() {
            let doc = entry.value();
            for spread in &doc.fragment_spreads {
                used.insert(spread.clone());
            }
        }
        used
    }

    async fn reload_schema(&self, changed_path: &str) {
        let mut sources_to_reload = Vec::new();
        for project in &self.config.projects {
            if project.schema.files().iter().any(|f| {
                let abs = self.config.base_dir.join(f);
                abs.to_string_lossy() == changed_path
                    || abs
                        .canonicalize()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
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
                        || abs
                            .canonicalize()
                            .ok()
                            .map(|p| p.to_string_lossy().to_string())
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
                    .log_message(
                        MessageType::INFO,
                        format!("Schema set {} successfully reloaded!", key),
                    )
                    .await;

                let mut to_publish = Vec::new();
                let used_fragments = self.get_used_fragments();
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
                            let fragment_names: Vec<_> = fragments.iter().map(|f| f.name.clone()).collect();
                            let diagnostics = doc.get_semantic_diagnostics(
                                &doc_schema,
                                &fragment_names,
                                Some(&used_fragments),
                                Some(&self.config),
                                false,
                            );
                            to_publish.push((uri.clone(), diagnostics));
                        }
                    }
                }
                for (uri, diagnostics) in to_publish {
                    self.client
                        .publish_diagnostics(uri, diagnostics, None)
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
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers })
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
                        let mut value = format!("```graphql\n{}\n```", info);
                        
                        if let Some(desc) = other_doc.find_description(&symbol_name) {
                            value.push_str("\n\n---\n");
                            value.push_str(&desc);
                        }

                        if !is_same_package {
                             if let Ok(other_p) = other_doc.uri.to_file_path() {
                                 if let Some(proj) = self.config.get_project_for_path(&other_p) {
                                     if let Some(import) = &proj.import {
                                         value.push_str("\n\n---\n");
                                         value.push_str(&format!("Import: `{}`", import));
                                     }
                                 }
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

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(uri) {
            let schema = self.get_schema_for_doc(uri);
            return Ok(doc.get_signature_help(position, &schema));
        }

        Ok(None)
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(uri) {
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

        for entry in self.documents.iter() {
            let doc = entry.value();
            let refs = doc.find_references_in_tree(&symbol_name, false);
            
            if !refs.is_empty() {
                // For each reference, we need to find the container (fragment or operation)
                // This is a bit expensive but necessary for call hierarchy.
                // For now, let's group by URI.
                
                let mut ranges_by_container: std::collections::HashMap<String, Vec<Range>> = std::collections::HashMap::new();
                
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
                            range: doc.find_definition_in_tree(&name).map(|l| l.range).unwrap_or(ranges[0]),
                            selection_range: doc.find_definition_in_tree(&name).map(|l| l.range).unwrap_or(ranges[0]),
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
        let uri = item.uri;

        if let Some(doc) = self.documents.get(&uri) {
            let mut calls = doc.get_outgoing_calls(&symbol_name);
            
            // Resolve the 'to' items
            for call in &mut calls {
                let callee_name = &call.to.name;
                // Find where it's defined
                for entry in self.documents.iter() {
                    let other_doc = entry.value();
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
        let used_fragments = self.get_used_fragments();
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let fragment_names: Vec<_> = fragments.iter().map(|f| f.name.clone()).collect();
            let diagnostics =
                doc.get_semantic_diagnostics(&schema, &fragment_names, Some(&used_fragments), Some(&self.config), false);
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
            let fragment_names: Vec<_> = fragments.iter().map(|f| f.name.clone()).collect();
            let diagnostics =
                other_doc.get_semantic_diagnostics(&schema, &fragment_names, Some(&used_fragments), Some(&self.config), false);
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

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let symbol_name = if let Some(doc) = self.documents.get(&uri) {
            doc.get_symbol_at_position(position)
        } else {
            None
        };

        if let Some(name) = symbol_name {
            let mut all_references = Vec::new();

            for entry in self.documents.iter() {
                let other_doc = entry.value();

                let refs = other_doc.find_references_in_tree(&name, include_declaration);
                all_references.extend(refs);
            }

            if all_references.is_empty() {
                return Ok(None);
            }

            return Ok(Some(all_references));
        }

        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let symbol_name = if let Some(doc) = self.documents.get(&uri) {
            doc.get_symbol_at_position(position)
        } else {
            None
        };

        if let Some(name) = symbol_name {
            let mut changes = std::collections::HashMap::new();

            for entry in self.documents.iter() {
                let other_uri = entry.key();
                let other_doc = entry.value();

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
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let position = params.position;

        if let Some(doc) = self.documents.get(&uri) {
            if let Some(_name) = doc.get_symbol_at_position(position) {
                // For now, we don't return the exact range, but just confirm it's renameable
                // You can improve this by returning the range of the symbol
                return Ok(Some(PrepareRenameResponse::DefaultBehavior {
                    default_behavior: true,
                }));
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
        let used_fragments = self.get_used_fragments();
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let fragment_names: Vec<_> = fragments.iter().map(|f| f.name.clone()).collect();
            let diagnostics =
                doc.get_semantic_diagnostics(&schema, &fragment_names, Some(&used_fragments), Some(&self.config), false);
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
            let fragment_names: Vec<_> = fragments.iter().map(|f| f.name.clone()).collect();
            let diagnostics =
                other_doc.get_semantic_diagnostics(&schema, &fragment_names, Some(&used_fragments), Some(&self.config), false);
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

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_lowercase();
        let mut all_symbols = Vec::new();

        for entry in self.documents.iter() {
            let doc = entry.value();
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
                }
            }
        }

        // 2. Selection-based refactors
        if let Some(doc) = self.documents.get(uri) {
            let extraction_actions = doc.get_extraction_actions(params.range);
            for action in extraction_actions {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
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
