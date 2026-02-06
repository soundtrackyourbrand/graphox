use criterion::{Criterion, criterion_group, criterion_main};
use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    document::DocumentState,
};
use std::path::Path;
use std::{fs, time::Duration};
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

fn generate_workspace(base_dir: &Path, projects_count: usize, files_per_project: usize) -> Config {
    let mut projects = Vec::new();

    for i in 0..projects_count {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        let schema_path = project_dir.join("schema.graphql");
        fs::write(
            &schema_path,
            "type User { id: ID! name: String } type Query { me: User }",
        )
        .unwrap();

        for j in 0..files_per_project {
            let file_path = project_dir.join(format!("file_{}.graphql", j));
            let content = format!(
                "query GetUser{}_{} {{ me {{ id name }} }}\nfragment UserInfo{}_{} on User {{ id }}",
                i, j, i, j
            );
            fs::write(file_path, content).unwrap();
        }

        projects.push(ProjectConfig {
            schema: SchemaSource::Single(format!("project_{}/schema.graphql", i)),
            include: GlobPattern::Single(format!("project_{}/**/*.graphql", i)),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        });
    }

    Config {
        projects,
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: None,
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        watch_all_files: None,
        output_dir: None,
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        timeouts: None,
        enable_schema_cache: Some(true),
    }
}

fn bench_lsp_actions(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // 10 projects, 500 files each = 5000 files
    let config = generate_workspace(&base_dir, 10, 500);

    // Enter the runtime context before creating Backend (which spawns tasks in CodegenThrottle::new)
    let _guard = rt.enter();
    let (service, _) = LspService::new(|client| Backend::new(client, config.clone()));
    let backend = service.inner();

    // Pre-populate documents to simulate an initialized LSP with a large workspace
    rt.block_on(async {
        let workspace_metadata = graphql_rust::engine::Engine::scan_workspace(&config, |_, _| {});
        for project_meta in workspace_metadata.projects {
            for file_path in project_meta.files {
                let abs_path = fs::canonicalize(&file_path).unwrap();
                let uri = Url::from_file_path(&abs_path).unwrap();
                let content = fs::read_to_string(&file_path).unwrap();
                let mut parser = tree_sitter::Parser::new();
                parser
                    .set_language(&tree_sitter_graphql::LANGUAGE.into())
                    .unwrap();
                let doc = DocumentState::new(uri.clone(), &content, parser);
                backend.documents.insert(uri, std::sync::Arc::new(doc));
            }
        }
    });

    let target_uri = Url::from_file_path(base_dir.join("project_0/file_0.graphql")).unwrap();

    let mut group = c.benchmark_group("LSP Actions (5000 files)");
    group.sample_size(10);

    group.bench_function("Hover", |b| {
        b.to_async(&rt).iter(|| {
            backend.hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, 20), // on 'me'
                },
                work_done_progress_params: Default::default(),
            })
        });
    });

    group.bench_function("Completion", |b| {
        b.to_async(&rt).iter(|| {
            backend.completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, 23), // after 'me {'
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
        });
    });

    group.bench_function("Go to Definition", |b| {
        b.to_async(&rt).iter(|| {
            backend.goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(1, 25), // on 'User' in fragment
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.bench_function("References", |b| {
        b.to_async(&rt).iter(|| {
            backend.references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(1, 10), // on fragment name
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
        });
    });

    group.bench_function("Document Symbols", |b| {
        b.to_async(&rt).iter(|| {
            backend.document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: target_uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.bench_function("Semantic Tokens", |b| {
        b.to_async(&rt).iter(|| {
            backend.semantic_tokens_full(SemanticTokensParams {
                text_document: TextDocumentIdentifier {
                    uri: target_uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.bench_function("Rename", |b| {
        b.to_async(&rt).iter(|| {
            backend.rename(RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(1, 10), // on fragment name
                },
                new_name: "RenamedFragment".to_string(),
                work_done_progress_params: Default::default(),
            })
        });
    });

    group.bench_function("Workspace Symbol", |b| {
        b.to_async(&rt).iter(|| {
            backend.symbol(WorkspaceSymbolParams {
                query: "UserInfo0_0".to_string(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.bench_function("Code Action", |b| {
        b.to_async(&rt).iter(|| {
            backend.code_action(CodeActionParams {
                text_document: TextDocumentIdentifier {
                    uri: target_uri.clone(),
                },
                range: Range::new(Position::new(0, 20), Position::new(0, 22)),
                context: CodeActionContext::default(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.bench_function("Run Codegen", |b| {
        b.to_async(&rt).iter(|| backend.run_codegen());
    });

    group.bench_function("Check Workspace (Full Diagnostics)", |b| {
        b.to_async(&rt).iter(|| async {
            let used_fragments = backend.get_used_fragments();
            for entry in backend.documents.iter() {
                let uri = entry.key();
                let doc = entry.value();
                let schema = backend.get_schema_for_doc(uri);
                let fragments = backend.get_fragments_for_doc(doc);
                let _diagnostics = doc.get_semantic_diagnostics(
                    &schema,
                    &fragments,
                    Some(&used_fragments),
                    Some(&backend.config.read().unwrap()),
                    false,
                    true,
                );
            }
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_millis(800)).measurement_time(Duration::from_millis(1000));
    targets = bench_lsp_actions
);
criterion_main!(benches);
