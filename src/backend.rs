use crate::Config;
use crate::config::SchemaSource;
use crate::document::{DocumentLanguage, DocumentState};
use crate::features::completion::FragmentCompletionInfo;
use crate::utils::{SEMANTIC_TOKEN_LEGEND, is_relevant_file};
use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use serde_json::Value;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};
use tree_sitter::StreamingIterator;

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
        }
    }

    fn load_schema_source(
        base_dir: &std::path::Path,
        source: &SchemaSource,
    ) -> Option<Arc<Schema>> {
        let mut texts = Vec::new();
        for file in source.files() {
            let path = base_dir.join(file);
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    texts.push(text);
                }
                Err(_) => return None,
            }
        }
        let combined_text = crate::utils::merge_schema_texts(&texts);
        Schema::parse(&combined_text, source.as_key())
            .ok()
            .map(Arc::new)
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
        if let Ok(path) = uri.to_file_path()
            && let Some(schema_path) = self.config.get_schema_for_path(&path)
            && let Some(schema) = self.validated_schemas.get(&schema_path)
        {
            return schema.value().clone();
        }

        self.valid_empty_schema.clone()
    }

    pub fn get_all_fragments_info(&self) -> Vec<FragmentCompletionInfo> {
        self.fragment_defs
            .iter()
            .flat_map(|entry| {
                let uri = entry.key();
                let frags = entry.value();

                let import_path = if let Ok(p) = uri.to_file_path() {
                    self.config
                        .get_project_for_path(&p)
                        .and_then(|proj| proj.import.clone())
                } else {
                    None
                };

                let package_root = self.package_roots.get(uri).and_then(|r| r.value().clone());

                frags
                    .iter()
                    .map(|frag| FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: frag.description.clone(),
                        import_path: import_path.clone(),
                        is_public: frag.is_public,
                        uri: uri.clone(),
                        package_root: package_root.clone(),
                        used_variables: frag.used_variables.clone(),
                        used_fragments: frag.used_fragments.clone(),
                        requirements: std::collections::BTreeMap::new(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn get_fragments_for_doc(&self, doc: &DocumentState) -> Vec<FragmentCompletionInfo> {
        let all_fragments = self.get_all_fragments_info();
        let target_package_root = doc.package_root.as_ref();

        all_fragments
            .into_iter()
            .filter(|f| {
                let is_same_package = f.package_root.as_ref() == target_package_root;
                is_same_package || f.is_public
            })
            .collect()
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
            }) {
                if let Some(doc) = self.documents.get(&frag.uri).map(|r| r.value().clone()) {
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
                if let Ok(valid) = (*new_schema).clone().validate() {
                    self.validated_schemas.insert(key.clone(), Arc::new(valid));
                }
                self.schemas.insert(key.clone(), new_schema.clone());
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("Schema set {} successfully reloaded!", key),
                    )
                    .await;

                let mut affected_by_schema = Vec::new();
                let all_uris: Vec<Url> = self.documents.iter().map(|e| e.key().clone()).collect();
                for uri in all_uris {
                    if let Ok(doc_path) = uri.to_file_path() {
                        if self
                            .config
                            .get_schema_for_path(&doc_path)
                            .is_some_and(|p| p.as_str() == key.as_str())
                        {
                            affected_by_schema.push(uri);
                        }
                    }
                }
                self.validate_uris(affected_by_schema).await;
            }
        }
    }

    async fn clear_cache(&self) {
        self.schemas.clear();
        self.validated_schemas.clear();

        // Reload project schemas from config
        for project in &self.config.projects {
            let key = project.schema.as_key();
            if !self.schemas.contains_key(&key)
                && let Some(schema) =
                    Self::load_schema_source(&self.config.base_dir, &project.schema)
            {
                if let Ok(valid) = (*schema).clone().validate() {
                    self.validated_schemas.insert(key.clone(), Arc::new(valid));
                }
                self.schemas.insert(key, schema);
            }
        }

        // Re-validate all open documents
        self.validate_all_documents().await;

        self.client
            .log_message(MessageType::INFO, "Cache cleared and schemas reloaded!")
            .await;
    }

    pub async fn run_codegen(&self) {
        Self::run_codegen_internal(self.client.clone(), self.config.clone()).await;
    }

    async fn run_codegen_internal(client: Client, config: Config) {
        let workspace_metadata = crate::engine::Engine::scan_workspace(&config, |_, _| {});

        let global_metadata = &workspace_metadata.fragments;
        let global_output_dir = config.output_dir.as_deref();
        let mut all_generated_operations = Vec::new();

        for (project, project_meta) in config
            .projects
            .iter()
            .zip(&workspace_metadata.projects)
        {
            let project_files = &project_meta.files;
            let project_output_dir = project.output_dir.as_deref().or(global_output_dir);

            let project_schema_files: fnv::FnvHashSet<_> =
                project.schema.files().into_iter().collect();
            let schema_import = config.schema_types.as_ref().and_then(|sts| {
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
                match crate::engine::Engine::load_schema(&config.base_dir, &project.schema) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = client.log_message(MessageType::ERROR, e).await;
                        continue;
                    }
                };

            let valid_schema = match schema.validate() {
                Ok(v) => v,
                Err(e) => {
                    let _ = client
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
                        scalars: &config.scalars,
                        schema_import: &schema_import,
                        generate_ast_for_fragments: config
                            .generate_ast_for_fragments
                            .unwrap_or(false),
                    };

                    if let Ok((ts_code, mut ops)) =
                        crate::features::codegen::generate_typescript(doc, &ctx)
                    {
                        let out_path = crate::utils::get_output_path(
                            path,
                            &config.base_dir,
                            project_output_dir,
                        );
                        let abs_out_path = if out_path.is_absolute() {
                            out_path
                        } else {
                            config.base_dir.join(out_path)
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
            let out_dir_path = config.base_dir.join(out_dir);
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

    fn update_dependency_indices(
        &self,
        uri: &Url,
        old_spreads: Option<Vec<String>>,
        new_spreads: Vec<String>,
    ) {
        if let Some(old) = old_spreads {
            for spread in old {
                if !new_spreads.contains(&spread)
                    && let Some(mut entry) = self.fragment_dependents.get_mut(&spread)
                {
                    entry.remove(uri);
                }
            }
        }

        for spread in new_spreads {
            self.fragment_dependents
                .entry(spread)
                .or_default()
                .insert(uri.clone());
        }
    }

    fn update_definition_indices(
        &self,
        uri: &Url,
        old_fragments: Option<Vec<String>>,
        new_fragments: Vec<String>,
    ) {
        if let Some(old) = old_fragments {
            for name in old {
                if !new_fragments.contains(&name)
                    && let Some(mut entry) = self.fragment_definitions.get_mut(&name)
                {
                    entry.remove(uri);
                }
            }
        }

        for name in new_fragments {
            self.fragment_definitions
                .entry(name)
                .or_default()
                .insert(uri.clone());
        }
    }

    pub async fn validate_uris(&self, uris: Vec<Url>) {
        if uris.is_empty() {
            return;
        }

        let mut to_publish = Vec::new();
        let used_fragments = self.get_used_fragments();
        let workspace_loaded = self.workspace_loaded.load(Ordering::SeqCst);

        // Pre-calculate all fragments info to avoid holding locks during O(N^2) work
        let all_fragments_info = self.get_all_fragments_info();

        for uri in uris {
            if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
                // Skip validating schema files as executable documents
                if let Ok(path) = uri.to_file_path() {
                    if let Some(schema_key) = self.config.get_schema_for_path(&path) {
                        if schema_key.contains(&path.to_string_lossy().to_string())
                            && !self.open_documents.contains(&uri)
                        {
                            continue;
                        }
                    }
                }

                let schema = self.get_schema_for_doc(&uri);

                // Filter fragments for this doc
                let target_package_root = doc.package_root.as_ref();
                let filtered_fragments: Vec<_> = all_fragments_info
                    .iter()
                    .filter(|f| {
                        let is_same_package = f.package_root.as_ref() == target_package_root;
                        is_same_package || f.is_public
                    })
                    .cloned()
                    .collect();

                let diagnostics = doc.get_semantic_diagnostics(
                    &schema,
                    &filtered_fragments,
                    Some(&used_fragments),
                    Some(&self.config),
                    false,
                    workspace_loaded,
                );
                to_publish.push((uri.clone(), diagnostics));
            }
        }

        for (u, d) in to_publish {
            self.client.publish_diagnostics(u, d, None).await;
        }
    }

    pub async fn validate_all_documents(&self) {
        let all_uris: Vec<Url> = self.documents.iter().map(|e| e.key().clone()).collect();
        self.validate_uris(all_uris).await;
    }

    fn get_affected_uris(
        &self,
        initial_uri: Url,
        affected_fragment_names: FnvHashSet<String>,
        affected_spread_names: FnvHashSet<String>,
    ) -> Vec<Url> {
        let mut uris_to_validate = FnvHashSet::default();
        uris_to_validate.insert(initial_uri);

        let mut to_process: Vec<String> = affected_fragment_names.into_iter().collect();
        let mut processed_fragments = FnvHashSet::default();

        while let Some(frag_name) = to_process.pop() {
            if !processed_fragments.insert(frag_name.clone()) {
                continue;
            }

            if let Some(dependents) = self.fragment_dependents.get(&frag_name) {
                for dep_uri in dependents.value() {
                    if uris_to_validate.insert(dep_uri.clone()) {
                        if let Some(doc) = self.documents.get(dep_uri).map(|r| r.value().clone()) {
                            for f in doc.fragments() {
                                to_process.push(f.name.clone());
                            }
                        }
                    }
                }
            }
        }

        for spread_name in affected_spread_names {
            if let Some(definitions) = self.fragment_definitions.get(&spread_name) {
                for def_uri in definitions.value() {
                    uris_to_validate.insert(def_uri.clone());
                }
            }
        }

        uris_to_validate.into_iter().collect()
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
        let client = self.client.clone();
        let config = self.config.clone();
        let documents = self.documents.clone();
        let fragment_defs = self.fragment_defs.clone();
        let fragment_spreads = self.fragment_spreads.clone();
        let package_roots = self.package_roots.clone();
        let fragment_dependents = self.fragment_dependents.clone();
        let fragment_definitions = self.fragment_definitions.clone();
        let workspace_loaded = self.workspace_loaded.clone();
        let empty_schema = self.empty_schema.clone();
        let schemas = self.schemas.clone();

        let workspace_scan_cancelled = self.workspace_scan_cancelled.clone();

        tokio::spawn(async move {
            let token = NumberOrString::String("workspace-scan".to_string());
            let cancelled = workspace_scan_cancelled;

            // Create progress in a separate task so it doesn't block the scan if the client doesn't respond to the request
            let client_clone = client.clone();
            let token_clone = token.clone();
            tokio::spawn(async move {
                let _ = client_clone
                    .send_request::<request::WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                        token: token_clone.clone(),
                    })
                    .await;

                // Begin progress
                let _ = client_clone
                    .send_notification::<notification::Progress>(ProgressParams {
                        token: token_clone,
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                            WorkDoneProgressBegin {
                                title: "Scanning workspace".to_string(),
                                cancellable: Some(true),
                                message: Some("Parsing GraphQL files...".to_string()),
                                percentage: Some(0),
                            },
                        )),
                    })
                    .await;
            });

            let workspace_metadata = crate::engine::Engine::scan_workspace_cancellable(
                &config,
                |_, doc| {
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }
                    let uri = doc.uri.clone();

                    fragment_defs.insert(uri.clone(), doc.fragments().to_vec());
                    fragment_spreads.insert(uri.clone(), doc.fragment_spreads.clone());
                    package_roots.insert(uri.clone(), doc.package_root.clone());

                    for frag in doc.fragments() {
                        fragment_definitions
                            .entry(frag.name.clone())
                            .or_default()
                            .insert(uri.clone());
                    }

                    // Also index type definitions (for Go to Definition)
                    let query = crate::queries::GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
                        let lang = tree_sitter_graphql::LANGUAGE.into();
                        tree_sitter::Query::new(&lang, crate::queries::GQL_DEFINITION_QUERY).unwrap()
                    });
                    let mut cursor = tree_sitter::QueryCursor::new();
                    for block in doc.get_graphql_trees() {
                        let mut matches = cursor.matches(
                            query,
                            block.tree.root_node(),
                            |node: tree_sitter::Node| {
                                doc.rope
                                    .byte_slice(
                                        (node.start_byte() + block.offset)
                                            ..(node.end_byte() + block.offset),
                                    )
                                    .chunks()
                            },
                        );
                        while let Some(m) = matches.next() {
                            let name_node = m.captures[0].node;
                            let name = doc.get_node_text(name_node, block.offset);
                            fragment_definitions
                                .entry(name)
                                .or_default()
                                .insert(uri.clone());
                        }
                    }

                    for spread in &doc.fragment_spreads {
                        fragment_dependents
                            .entry(spread.clone())
                            .or_default()
                            .insert(uri.clone());
                    }

                    // If the document is not already open, we still might want to keep it in memory
                    // for fast definition/hover/etc.
                    if !documents.contains_key(&uri) {
                        documents.insert(uri, Arc::new(doc));
                    }
                },
                |current, total| {
                    if total == 0 {
                        return;
                    }
                    let percentage = (current * 100 / total) as u32;
                    let client = client.clone();
                    let token = token.clone();
                    tokio::spawn(async move {
                        let _ = client
                            .send_notification::<notification::Progress>(ProgressParams {
                                token,
                                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                                    WorkDoneProgressReport {
                                        cancellable: Some(true),
                                        message: Some(format!(
                                            "Parsing GraphQL files... ({}/{})",
                                            current, total
                                        )),
                                        percentage: Some(percentage),
                                    },
                                )),
                            })
                            .await;
                    });
                },
                cancelled.clone(),
            );
            let total_docs = workspace_metadata.documents.len();

            workspace_loaded.store(true, Ordering::SeqCst);

            // Re-validate all documents
            let used_fragments = {
                let mut used = fnv::FnvHashSet::default();
                for entry in fragment_spreads.iter() {
                    for spread in entry.value() {
                        used.insert(spread.clone());
                    }
                }
                used
            };

            // Pre-calculate validated schemas to avoid repeated validation
            let mut validated_schemas_map = fnv::FnvHashMap::default();
            for entry in schemas.iter() {
                if let Ok(valid) = (**entry.value()).clone().validate() {
                    validated_schemas_map.insert(entry.key().clone(), Arc::new(valid));
                }
            }
            let valid_empty_schema = Arc::new((*empty_schema).clone().validate().unwrap());

            // Pre-calculate all fragments info
            let all_fragments_info: Vec<FragmentCompletionInfo> = fragment_defs
                .iter()
                .flat_map(|entry| {
                    let uri = entry.key();
                    let frags = entry.value();

                    let import_path = if let Ok(p) = uri.to_file_path() {
                        config
                            .get_project_for_path(&p)
                            .and_then(|proj| proj.import.clone())
                    } else {
                        None
                    };

                    let package_root =
                        package_roots.get(uri).and_then(|r| r.value().clone());

                    frags
                        .iter()
                        .map(move |frag| {
                            FragmentCompletionInfo {
                                name: frag.name.clone(),
                                type_condition: frag.type_condition.clone(),
                                description: frag.description.clone(),
                                import_path: import_path.clone(),
                                is_public: frag.is_public,
                                uri: uri.clone(),
                                package_root: package_root.clone(),
                                used_variables: frag.used_variables.clone(),
                                used_fragments: frag.used_fragments.clone(),
                                requirements: std::collections::BTreeMap::new(),
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();

            let to_publish: Vec<(Url, Vec<Diagnostic>)> = documents
                .as_ref()
                .par_iter()
                .map(|entry| {
                    let uri = entry.key();
                    let doc = entry.value();

                    // Get schema for doc
                    let schema: Arc<apollo_compiler::validation::Valid<Schema>> = if let Ok(path) = uri.to_file_path()
                        && let Some(schema_path) = config.get_schema_for_path(&path)
                        && let Some(schema) = validated_schemas_map.get(&schema_path)
                    {
                        schema.clone()
                    } else {
                        valid_empty_schema.clone()
                    };

                    // Filter fragments for this doc (same package or public)
                    let target_package_root = doc.package_root.as_ref();
                    let filtered_fragments: Vec<_> = all_fragments_info
                        .iter()
                        .filter(|f| {
                            let is_same_package = f.package_root.as_ref() == target_package_root;
                            is_same_package || f.is_public
                        })
                        .cloned()
                        .collect();

                    let diagnostics = doc.get_semantic_diagnostics(
                        &schema,
                        &filtered_fragments,
                        Some(&used_fragments),
                        Some(&config),
                        false,
                        true,
                    );
                    (uri.clone(), diagnostics)
                })
                .collect();

            for (u, d) in to_publish {
                client.publish_diagnostics(u, d, None).await;
            }

            // End progress
            let _ = client
                .send_notification::<notification::Progress>(ProgressParams {
                    token,
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                        WorkDoneProgressEnd {
                            message: Some(format!("Finished scanning {} files", total_docs)),
                        },
                    )),
                })
                .await;

            client
                .log_message(MessageType::INFO, "Workspace scan complete.")
                .await;
        });

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

        if self.config.watch_all_files() {
            // Watch all relevant files in the workspace
            watchers.push(FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.{graphql,gql,ts,tsx,mts,cts,js,jsx,mjs,cjs}".to_string()),
                kind: Some(WatchKind::all()),
            });
        }

        let registration = Registration {
            id: "watch-files".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers })
                    .unwrap(),
            ),
        };
        let client_clone = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client_clone.register_capability(vec![registration]).await {
                client_clone
                    .log_message(
                        MessageType::ERROR,
                        format!("Failed to register schema watcher: {}", e),
                    )
                    .await;
            }
        });
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
                                if let Some(proj) = self.config.get_project_for_path(&other_p) {
                                    if let Some(import) = &proj.import {
                                        value.push_str("\n\n---\n");
                                        value.push_str(&format!("Import: `{}`", import));
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

        for entry in self.documents.iter() {
            let doc = entry.value();
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
            tokio::spawn(async move {
                Self::run_codegen_internal(client, config).await;
            });
        }
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri);
        self.open_documents.remove(&uri);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = self.normalize_uri(params.text_document.uri.clone());

        let mut affected_fragment_names = FnvHashSet::default();
        let mut old_fragment_names = Vec::new();
        let old_spreads: Vec<String>;

        let new_fragments: Vec<crate::document::FragmentDef>;
        let new_spreads: Vec<String>;
        let package_root: Option<PathBuf>;
        let new_fragment_names: Vec<String>;
        let affected_spread_names: FnvHashSet<String>;

        if let Some(doc_arc) = self.documents.get(&uri).map(|r| r.value().clone()) {
            let mut doc = (*doc_arc).clone();

            // Collect fragments before change
            for f in doc.fragments() {
                affected_fragment_names.insert(f.name.clone());
                old_fragment_names.push(f.name.clone());
            }

            old_spreads = doc.fragment_spreads.clone();

            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&doc.language.get_parser_language())
                .unwrap();

            for change in params.content_changes {
                doc.apply_change(&change, &mut parser);
            }

            // Collect fragments after change
            for f in doc.fragments() {
                affected_fragment_names.insert(f.name.clone());
            }

            new_fragments = doc.fragments().to_vec();
            new_spreads = doc.fragment_spreads.clone();

            let mut aff_spreads = FnvHashSet::default();
            for s in &old_spreads {
                if !new_spreads.contains(s) {
                    aff_spreads.insert(s.clone());
                }
            }
            for s in &new_spreads {
                if !old_spreads.contains(s) {
                    aff_spreads.insert(s.clone());
                }
            }
            affected_spread_names = aff_spreads;
            package_root = doc.package_root.clone();
            new_fragment_names = doc.fragments().iter().map(|f| f.name.clone()).collect();

            self.documents.insert(uri.clone(), Arc::new(doc));
        } else {
            return;
        }

        self.fragment_defs.insert(uri.clone(), new_fragments);
        self.fragment_spreads
            .insert(uri.clone(), new_spreads.clone());
        self.update_dependency_indices(&uri, Some(old_spreads), new_spreads);
        self.update_definition_indices(&uri, Some(old_fragment_names), new_fragment_names);
        self.package_roots.insert(uri.clone(), package_root);

        let uris_to_validate =
            self.get_affected_uris(uri, affected_fragment_names, affected_spread_names);
        self.validate_uris(uris_to_validate).await;

        if self.config.lsp_automatic_codegen() {
            let client = self.client.clone();
            let config = self.config.clone();
            tokio::spawn(async move {
                Self::run_codegen_internal(client, config).await;
            });
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

                let mut preferred_uris = Vec::new();
                if let Ok(path) = uri.to_file_path() {
                    if let Some(project) = self.config.get_project_for_path(&path) {
                        for schema_file in project.schema.files() {
                            let schema_path = self.config.base_dir.join(schema_file);
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

                let symbol_name = doc_arc.get_symbol_at_position(position);

                if let Some(name) = symbol_name {
                    if name.starts_with('$') {
                        if let Some(location) = doc_arc.find_variable_definition(&name, position) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                        }
                        return Ok(None);
                    }

                    // Targeted lookup using the index
                    if let Some(uris) = self.fragment_definitions.get(&name) {
                        for other_uri in uris.iter() {
                            if let Some(other_doc) = self.documents.get(&*other_uri).map(|r| r.value().clone()) {
                                let is_same_package = other_doc.package_root == doc_arc.package_root;
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

            let symbol_name = if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
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
                                if let Some(f_doc) = self.documents.get(&f_uri).map(|r| r.value().clone()) {
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

            let symbol_name = if let Some(doc) = self.documents.get(&uri).map(|r| r.value().clone()) {
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

                let doc_arcs: Vec<(Url, Arc<DocumentState>)> =
                    self.documents.iter().map(|e| (e.key().clone(), e.value().clone())).collect();

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

        // 2. Refactoring actions
        if let Some(doc) = self.documents.get(uri).map(|r| r.value().clone())
 {
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
                if change.typ == FileChangeType::CREATED || change.typ == FileChangeType::CHANGED {
                    let path = change.uri.to_file_path().unwrap();
                    let path_str = path.to_string_lossy().to_string();

                    // Check if this is a schema file
                    let mut is_schema = false;
                    for project in &self.config.projects {
                        if project.schema.files().iter().any(|f| {
                            let abs = self.config.base_dir.join(f);
                            abs.to_string_lossy() == path_str
                                || abs
                                    .canonicalize()
                                    .ok()
                                    .map(|p| p.to_string_lossy().to_string())
                                    == Some(path_str.clone())
                        }) {
                            is_schema = true;
                            break;
                        }
                    }

                    if !is_schema
                        && let Some(schema_types) = &self.config.schema_types
                    {
                        for st in schema_types {
                            if st.schema.files().iter().any(|f| {
                                let abs = self.config.base_dir.join(f);
                                abs.to_string_lossy() == path_str
                                    || abs
                                        .canonicalize()
                                        .ok()
                                        .map(|p| p.to_string_lossy().to_string())
                                        == Some(path_str.clone())
                            }) {
                                is_schema = true;
                                break;
                            }
                        }
                    }

                    if is_schema {
                        self.reload_schema(&path_str).await;
                    } else if is_relevant_file(&path) && !crate::utils::is_path_ignored(&path, &self.gitignore) {
                        // Update document if it's already open
                        let uri = self.normalize_uri(change.uri);

                        let mut affected_fragment_names = FnvHashSet::default();
                        let mut affected_spread_names = FnvHashSet::default();

                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let language = DocumentLanguage::from_uri(&uri);
                            let mut parser = tree_sitter::Parser::new();
                            parser
                                .set_language(&language.get_parser_language())
                                .unwrap();
                            let new_doc = DocumentState::new(uri.clone(), &content, parser);

                            let old_fragments = self
                                .fragment_defs
                                .get(&uri)
                                .map(|f| f.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
                            let old_spreads = self.fragment_spreads.get(&uri).map(|s| s.clone());

                            let new_fragment_defs = new_doc.fragments().to_vec();
                            let new_fragment_names: Vec<_> =
                                new_fragment_defs.iter().map(|f| f.name.clone()).collect();
                            let new_spreads = new_doc.fragment_spreads.clone();

                            // Track changes to fragment definitions
                            if let Some(old) = &old_fragments {
                                for name in old {
                                    if !new_fragment_names.contains(name) {
                                        affected_fragment_names.insert(name.clone());
                                    }
                                }
                            }
                            for name in &new_fragment_names {
                                if old_fragments.as_ref().is_none_or(|old| !old.contains(name)) {
                                    affected_fragment_names.insert(name.clone());
                                }
                            }

                            // Track changes to fragment spreads
                            if let Some(old) = &old_spreads {
                                for name in old {
                                    if !new_spreads.contains(name) {
                                        affected_spread_names.insert(name.clone());
                                    }
                                }
                            }
                            for name in &new_spreads {
                                if old_spreads.as_ref().is_none_or(|old| !old.contains(name)) {
                                    affected_spread_names.insert(name.clone());
                                }
                            }

                            // Update metadata
                            self.fragment_defs.insert(uri.clone(), new_fragment_defs);
                            self.fragment_spreads
                                .insert(uri.clone(), new_spreads.clone());
                            self.package_roots
                                .insert(uri.clone(), new_doc.package_root.clone());

                            self.update_definition_indices(
                                &uri,
                                old_fragments,
                                new_fragment_names,
                            );
                            self.update_dependency_indices(&uri, old_spreads, new_spreads);

                    // Update documents map if we have it
                    if self.documents.contains_key(&uri) {
                        self.documents.insert(uri.clone(), Arc::new(new_doc));
                    }

                            let uris_to_validate = self.get_affected_uris(
                                uri,
                                affected_fragment_names,
                                affected_spread_names,
                            );
                            self.validate_uris(uris_to_validate).await;

                            if self.config.lsp_automatic_codegen() {
                                let client = self.client.clone();
                                let config = self.config.clone();
                                tokio::spawn(async move {
                                    Self::run_codegen_internal(client, config).await;
                                });
                            }
                        }
                    }
                } else if change.typ == FileChangeType::DELETED {
                    let uri = self.normalize_uri(change.uri);
                    let mut affected_fragment_names = FnvHashSet::default();
                    let mut affected_spread_names = FnvHashSet::default();

                    let old_fragments = self
                        .fragment_defs
                        .get(&uri)
                        .map(|f| f.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
                    let old_spreads = self.fragment_spreads.get(&uri).map(|s| s.clone());

                    if let Some(old) = &old_fragments {
                        for name in old {
                            affected_fragment_names.insert(name.clone());
                        }
                    }
                    if let Some(old) = &old_spreads {
                        for name in old {
                            affected_spread_names.insert(name.clone());
                        }
                    }

                    // Clean up metadata
                    self.documents.remove(&uri);
                    self.fragment_defs.remove(&uri);
                    self.fragment_spreads.remove(&uri);
                    self.package_roots.remove(&uri);
                    self.update_definition_indices(&uri, old_fragments, vec![]);
                    self.update_dependency_indices(&uri, old_spreads, vec![]);

                    let uris_to_validate =
                        self.get_affected_uris(uri, affected_fragment_names, affected_spread_names);
                    self.validate_uris(uris_to_validate).await;
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
