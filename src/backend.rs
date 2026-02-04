use crate::Config;
use crate::config::SchemaSource;
use crate::document::DocumentState;
use crate::features::completion::FragmentCompletionInfo;
use crate::utils::SEMANTIC_TOKEN_LEGEND;
use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use std::sync::Arc;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};
use serde_json::Value;

pub struct Backend {
    pub client: Client,
    pub documents: DashMap<Url, DocumentState, ahash::RandomState>,
    pub config: Config,
    pub schemas: DashMap<String, Arc<Schema>, ahash::RandomState>,
    pub empty_schema: Arc<Schema>,
    // Performance optimizations
    pub fragment_defs: DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>,
    pub fragment_spreads: DashMap<Url, Vec<String>, ahash::RandomState>,
    pub package_roots: DashMap<Url, Option<std::path::PathBuf>, ahash::RandomState>,
    pub fragment_dependents: DashMap<String, FnvHashSet<Url>, ahash::RandomState>,
}

impl Backend {
    pub fn new(client: Client, config: Config) -> Self {
        let schemas = DashMap::with_hasher(ahash::RandomState::default());
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
            documents: DashMap::with_hasher(ahash::RandomState::default()),
            config,
            schemas,
            empty_schema,
            fragment_defs: DashMap::with_hasher(ahash::RandomState::default()),
            fragment_spreads: DashMap::with_hasher(ahash::RandomState::default()),
            package_roots: DashMap::with_hasher(ahash::RandomState::default()),
            fragment_dependents: DashMap::with_hasher(ahash::RandomState::default()),
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

    pub fn get_schema_for_doc(&self, uri: &Url) -> Arc<Schema> {
        if let Ok(path) = uri.to_file_path()
            && let Some(schema_path) = self.config.get_schema_for_path(&path)
            && let Some(schema) = self.schemas.get(&schema_path)
        {
            return schema.value().clone();
        }

        self.empty_schema.clone()
    }

    pub fn get_fragments_for_doc(&self, doc: &DocumentState) -> Vec<FragmentCompletionInfo> {
        let mut fragments = Vec::new();
        let target_package_root = doc.package_root.as_ref();

        for entry in self.fragment_defs.iter() {
            let other_uri = entry.key();
            let other_frags = entry.value();

            let is_same_package = self.package_roots.get(other_uri).map(|pr| pr.as_ref() == target_package_root).unwrap_or(false);

            let import_path = if is_same_package {
                None
            } else if let Ok(p) = other_uri.to_file_path() {
                self.config.get_project_for_path(&p).and_then(|proj| proj.import.clone())
            } else {
                None
            };

            for frag in other_frags {
                if is_same_package || frag.is_public {
                    fragments.push(FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: frag.description.clone(),
                        import_path: import_path.clone(),
                        is_public: frag.is_public,
                        uri: other_uri.clone(),
                    });
                }
            }
        }
        fragments
    }

    pub fn get_used_fragments(&self) -> fnv::FnvHashSet<String> {
        let mut used = fnv::FnvHashSet::default();
        for entry in self.fragment_spreads.iter() {
            for spread in entry.value() {
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
                            let diagnostics = doc.get_semantic_diagnostics(
                                &doc_schema,
                                &fragments,
                                Some(&used_fragments),
                                Some(&self.config),
                                false,
                                Some(&self.package_roots),
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

    async fn clear_cache(&self) {
        self.schemas.clear();

        // Reload project schemas from config
        for project in &self.config.projects {
            let key = project.schema.as_key();
            if !self.schemas.contains_key(&key)
                && let Some(schema) = Self::load_schema_source(&self.config.base_dir, &project.schema)
            {
                self.schemas.insert(key, schema);
            }
        }

        // Re-validate all open documents
        let mut to_publish = Vec::new();
        let used_fragments = self.get_used_fragments();
        for entry in self.documents.iter() {
            let uri = entry.key();
            let doc = entry.value();
            let schema = self.get_schema_for_doc(uri);
            let fragments = self.get_fragments_for_doc(doc);
            let diagnostics = doc.get_semantic_diagnostics(
                &schema,
                &fragments,
                Some(&used_fragments),
                Some(&self.config),
                false,
                Some(&self.package_roots),
            );
            to_publish.push((uri.clone(), diagnostics));
        }

        for (uri, diagnostics) in to_publish {
            self.client.publish_diagnostics(uri, diagnostics, None).await;
        }

        self.client
            .log_message(MessageType::INFO, "Cache cleared and schemas reloaded!")
            .await;
    }

    pub async fn run_codegen(&self) {
        let workspace_metadata = crate::engine::Engine::scan_workspace(&self.config);
        let global_metadata = &workspace_metadata.fragments;
        let global_output_dir = self.config.output_dir.as_deref();
        let mut all_generated_operations = Vec::new();

        for (project, project_meta) in self.config.projects.iter().zip(&workspace_metadata.projects) {
            let project_files = &project_meta.files;
            let project_output_dir = project.output_dir.as_deref().or(global_output_dir);

            let project_schema_files: fnv::FnvHashSet<_> =
                project.schema.files().into_iter().collect();
            let schema_import = self.config.schema_types.as_ref().and_then(|sts| {
                let mut matches: Vec<_> = sts
                    .iter()
                    .filter(|st| {
                        let st_files = st.schema.files();
                        st_files.iter().all(|f| project_schema_files.contains(f))
                    })
                    .collect();

                matches.sort_by_key(|st| std::cmp::Reverse(st.schema.files().len()));
                matches.first().and_then(|st| st.import.clone())
            });

            let schema =
                match crate::engine::Engine::load_schema(&self.config.base_dir, &project.schema) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = self.client.log_message(MessageType::ERROR, e).await;
                        continue;
                    }
                };

            let valid_schema = match schema.validate() {
                Ok(v) => v,
                Err(e) => {
                    let _ = self
                        .client
                        .log_message(
                            MessageType::ERROR,
                            format!(
                                "Schema validation failed for project {}: {}",
                                project.include.as_key(),
                                e
                            ),
                        )
                        .await;
                    continue;
                }
            };

            let project_context = crate::engine::Engine::resolve_project_context(
                &valid_schema,
                global_metadata,
                project_files,
            );

            for path in project_files {
                if let Some(doc) = workspace_metadata.documents.get(path) {
                    if doc.get_graphql_trees().is_empty() {
                        continue;
                    }

                    let ctx = crate::features::codegen::CodegenContext {
                        schema: &valid_schema,
                        fragment_to_path: &project_context.fragment_to_path,
                        fragment_to_import: &project_context.fragment_to_import,
                        all_fragments: &project_context.all_fragments,
                        current_file_path: path,
                        scalars: &self.config.scalars,
                        schema_import: &schema_import,
                        generate_ast_for_fragments: self
                            .config
                            .generate_ast_for_fragments
                            .unwrap_or(false),
                    };

                    if let Ok((ts_code, mut ops)) =
                        crate::features::codegen::generate_typescript(doc, &ctx)
                    {
                        let out_path = crate::utils::get_output_path(path, &self.config.base_dir, project_output_dir);
                        let abs_out_path = if out_path.is_absolute() {
                            out_path
                        } else {
                            self.config.base_dir.join(out_path)
                        };

                        if let Some(parent) = abs_out_path.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }

                        if std::fs::write(&abs_out_path, ts_code).is_ok() {
                            for op in &mut ops {
                                op.codegen_path = abs_out_path.clone();
                            }
                            all_generated_operations.extend(ops);
                        }
                    }
                }
            }
        }

        if let Some(out_dir) = global_output_dir {
            let out_dir_path = self.config.base_dir.join(out_dir);
            let entrypoint_path = out_dir_path.join("graphql.ts");
            if !all_generated_operations.is_empty() {
                let content = crate::features::codegen::generate_entrypoint_content(
                    &out_dir_path,
                    &all_generated_operations,
                );
                let _ = std::fs::write(entrypoint_path, content);
            }
        }
    }

    fn update_dependency_indices(&self, uri: &Url, old_spreads: Option<Vec<String>>, new_spreads: Vec<String>) {
        if let Some(old) = old_spreads {
            for spread in old {
                if !new_spreads.contains(&spread) {
                    if let Some(mut entry) = self.fragment_dependents.get_mut(&spread) {
                        entry.remove(uri);
                    }
                }
            }
        }

        for spread in new_spreads {
            self.fragment_dependents.entry(spread).or_default().insert(uri.clone());
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
        
        // Update performance indices
        self.fragment_defs.insert(uri.clone(), doc.fragments().to_vec());
        self.fragment_spreads.insert(uri.clone(), doc.fragment_spreads.clone());
        self.package_roots.insert(uri.clone(), doc.package_root.clone());
        self.update_dependency_indices(&uri, None, doc.fragment_spreads.clone());
        
        self.documents.insert(uri.clone(), doc);

        let mut to_publish = Vec::new();
        let used_fragments = self.get_used_fragments();
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let diagnostics =
                doc.get_semantic_diagnostics(&schema, &fragments, Some(&used_fragments), Some(&self.config), false, Some(&self.package_roots));
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
                other_doc.get_semantic_diagnostics(&schema, &fragments, Some(&used_fragments), Some(&self.config), false, Some(&self.package_roots));
            to_publish.push((other_uri.clone(), diagnostics));
        }

        for (u, d) in to_publish {
            self.client.publish_diagnostics(u, d, None).await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        let mut modified_fragments = Vec::new();

        if let Some(mut doc) = self.documents.get_mut(&uri) {
            let old_fragments = doc.fragments().to_vec();
            let old_spreads = doc.fragment_spreads.clone();

            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&doc.language.get_parser_language())
                .unwrap();

            for change in params.content_changes {
                doc.apply_change(&change, &mut parser);
            }
            
            let new_fragments = doc.fragments().to_vec();
            let new_spreads = doc.fragment_spreads.clone();

            // Detect modified fragments
            for nf in &new_fragments {
                let found = old_fragments.iter().find(|of| of.name == nf.name);
                match found {
                    Some(of) if of.source_hash != nf.source_hash || of.type_condition != nf.type_condition => {
                        modified_fragments.push(nf.name.clone());
                    }
                    None => {
                        modified_fragments.push(nf.name.clone());
                    }
                    _ => {}
                }
            }
            // Also detect deleted fragments
            for of in &old_fragments {
                if !new_fragments.iter().any(|nf| nf.name == of.name) {
                    modified_fragments.push(of.name.clone());
                }
            }

            // Update performance indices
            self.fragment_defs.insert(uri.clone(), new_fragments);
            self.fragment_spreads.insert(uri.clone(), new_spreads.clone());
            self.package_roots.insert(uri.clone(), doc.package_root.clone());
            self.update_dependency_indices(&uri, Some(old_spreads), new_spreads);
        }

        let mut to_publish = Vec::new();
        let used_fragments = self.get_used_fragments();
        
        // 1. Re-validate the changed document
        if let Some(doc) = self.documents.get(&uri) {
            let schema = self.get_schema_for_doc(&uri);
            let fragments = self.get_fragments_for_doc(&doc);
            let diagnostics =
                doc.get_semantic_diagnostics(&schema, &fragments, Some(&used_fragments), Some(&self.config), false, Some(&self.package_roots));

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
                    other_doc.get_semantic_diagnostics(&schema, &fragments, Some(&used_fragments), Some(&self.config), false, Some(&self.package_roots));

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
            let schema = self.get_schema_for_doc(uri);
            let extraction_actions = doc.get_extraction_actions(params.range, &schema);
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

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
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
    }
}
