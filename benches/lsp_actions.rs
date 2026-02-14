use criterion::{Criterion, criterion_group, criterion_main};
use graphox::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    document::DocumentState,
    engine,
    features::diagnostics::DocumentDiagnostics,
};
use std::path::Path;
use std::{fs, time::Duration};
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

fn generate_workspace(base_dir: &Path, projects_count: usize, files_per_project: usize) -> Config {
    let mut projects = Vec::new();
    let type_count = 50; // Smaller schema
    let fields_per_type = 20;
    let max_nesting = 3;
    let fragments_per_file = 2;

    for i in 0..projects_count {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        let schema_path = project_dir.join("schema.graphql");
        let mut schema_content = String::new();

        // Generate 50 types with 20 fields each
        for t in 0..type_count {
            schema_content.push_str(&format!("type Type{} {{\n", t));
            for f in 0..fields_per_type {
                schema_content.push_str(&format!("  field_{}: String\n", f));
            }
            schema_content.push_str("}\n");
        }

        // Generate Query type with getters for each type
        schema_content.push_str("type Query {\n");
        for t in 0..type_count {
            schema_content.push_str(&format!("  getType{}: Type{}\n", t, t));
        }
        // Add a large union
        schema_content.push_str("  search: SearchResult\n");
        schema_content.push_str("}\n");

        // Large union with all types
        schema_content.push_str("union SearchResult = ");
        for t in 0..type_count {
            if t > 0 {
                schema_content.push_str(" | ");
            }
            schema_content.push_str(&format!("Type{}", t));
        }
        schema_content.push('\n');

        schema_content.push_str("schema { query: Query }\n");

        fs::write(&schema_path, schema_content).unwrap();

        // Generate files with queries and cross-file fragments
        // Each file's fragments spread fragments from OTHER files
        // This creates expensive cross-project dependency resolution
        for j in 0..files_per_project {
            let file_path = project_dir.join(format!("file_{}.graphql", j));

            // Calculate nesting depth based on file index
            let nesting = j % max_nesting;

            // Generate query at the specified nesting depth
            let query = generate_nested_query(nesting, max_nesting);

            // Generate 20 fragments that spread fragments from OTHER files
            // Project 0 has self-contained fragments (base case)
            // Projects 1+ spread fragments from project 0 (cross-project resolution)
            // This creates expensive cross-project dependency resolution
            let mut fragments = String::new();
            for f in 0..fragments_per_file {
                if i == 0 {
                    // Project 0: base fragments, no cross-project spreads
                    fragments.push_str(&format!(
                        "\nfragment Frag{}_{}_{} on Type{} {{ field_{} }}",
                        i,
                        j,
                        f,
                        f % type_count,
                        f % fields_per_type
                    ));
                } else {
                    // Projects 1+: spread fragments from project 0 (all cross-project)
                    fragments.push_str(&format!(
                        "\nfragment Frag{}_{}_{} on Type{} {{ ...Frag0_{}_{} }}",
                        i,
                        j,
                        f,
                        f % type_count,
                        j % files_per_project,
                        f
                    ));
                }
            }

            let content = format!("{}{}", query, fragments);
            fs::write(file_path, content).unwrap();
        }

        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Single(format!(
                    "project_{}/schema.graphql",
                    i
                )))
                .with_include(GlobPattern::Single(format!("project_{}/**/*.graphql", i))),
        );
    }

    Config::new_empty()
        .with_projects(projects)
        .with_base_dir(base_dir.to_path_buf())
        .with_enable_schema_cache(true)
}

fn generate_nested_query(nesting: usize, _max_nesting: usize) -> String {
    let mut query = String::from("query Q { ");

    // Build nested selection: getType0 -> getType1 -> ... -> getType{nesting}
    for i in 0..nesting {
        query.push_str(&format!("getType{} {{ ", i + 450));
    }

    // Add the final field from the deepest nested type
    query.push_str("field_0");

    // Close all the opening braces
    for _ in 0..nesting {
        query.push('}');
    }

    query.push('}');
    query
}

fn calculate_completion_position(nesting: usize) -> u32 {
    let prefix = "query Q { ";
    let segment = "getType0 { ";
    prefix.len() as u32 + (nesting as u32 * segment.len() as u32)
}

fn bench_lsp_actions(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // 10 projects, 50 files each = 500 files
    // 50 types × 20 fields each
    // 2 fragments per file = 1,000 total fragments (cross-project spreads)
    // Large union with 50 member types
    let config = generate_workspace(&base_dir, 10, 50);

    // Enter the runtime context before creating Backend (which spawns tasks in CodegenThrottle::new)
    let _guard = rt.enter();
    let (service, _) = LspService::new(|client| Backend::new(client, config.clone()));
    let backend = service.inner();

    // Pre-populate documents to simulate an initialized LSP with a large workspace
    rt.block_on(async {
        let workspace_metadata =
            engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
        for project_meta in workspace_metadata.projects {
            for file_path in project_meta.files {
                let abs_path = fs::canonicalize(&file_path).unwrap();
                let uri = Url::from_file_path(&abs_path).unwrap();
                let content = fs::read_to_string(&file_path).unwrap();
                let mut parser = tree_sitter::Parser::new();
                parser
                    .set_language(&tree_sitter_graphql::LANGUAGE.into())
                    .unwrap();
                let doc =
                    DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);
                backend.documents.insert(uri, std::sync::Arc::new(doc));
            }
        }
    });

    let target_uri = Url::from_file_path(base_dir.join("project_0/file_0.graphql")).unwrap();

    let mut group =
        c.benchmark_group("LSP Actions (500 files, 50 types × 20 fields, union, 1000 fragments)");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(5));

    group.bench_function("Hover", |b| {
        b.to_async(&rt).iter(|| {
            backend.hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, 18), // on 'getType0'
                },
                work_done_progress_params: Default::default(),
            })
        });
    });

    group.bench_function("Completion (level 0)", |b| {
        b.to_async(&rt).iter(|| {
            backend.completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, calculate_completion_position(0)),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
        });
    });

    group.bench_function("Completion (level 4)", |b| {
        b.to_async(&rt).iter(|| {
            backend.completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, calculate_completion_position(4)),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
        });
    });

    // Union completion: inside { search { ... } } - tests field merging across union members
    group.bench_function("Completion (union)", |b| {
        b.to_async(&rt).iter(|| {
            backend.completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, 16), // inside { search { ... } }
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
                    position: Position::new(0, 18), // on 'getType0'
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
            let config_val = backend.config.read().unwrap().clone();
            let all_uris: Vec<Url> = backend.documents.iter().map(|e| e.key().clone()).collect();
            let fragment_defs = backend.fragment_defs.clone();
            let fragment_spreads = backend.fragment_spreads.clone();
            let package_roots = backend.package_roots.clone();
            let validated_schemas = backend.validated_schemas.clone();
            let valid_empty_schema = backend.valid_empty_schema.clone();
            let workspace_loaded = backend.workspace_loaded.clone();
            let open_documents = backend.open_documents.clone();
            let fragment_dependents = backend.fragment_dependents.clone();
            let fragment_definitions = backend.fragment_definitions.clone();
            let operation_names = backend.operation_names.clone();

            let params = graphox_lsp::backend::validation::ValidationParams {
                client: &backend.client,
                documents: &backend.documents,
                config: &config_val,
                fragment_defs: &fragment_defs,
                fragment_spreads: &fragment_spreads,
                package_roots: &package_roots,
                validated_schemas: &validated_schemas,
                valid_empty_schema: &valid_empty_schema,
                workspace_loaded: &workspace_loaded,
                open_documents: &open_documents,
                fragment_dependents: &fragment_dependents,
                fragment_definitions: &fragment_definitions,
                operation_names: &operation_names,
                supports_progress: false,
                position_encoding: PositionEncodingKind::UTF16,
            };

            graphox_lsp::backend::validation::validate_uris(params, all_uris, false, None).await;
        });
    });

    // Single document diagnostics (matches user's textDocument/diagnostic call)
    group.bench_function("Document Diagnostic", |b| {
        b.to_async(&rt).iter(|| async {
            let doc = backend
                .documents
                .get(&target_uri)
                .map(|r| r.value().clone())
                .unwrap();
            let schema = backend.get_schema_for_doc(&target_uri);
            let all_fragments = backend.get_all_fragments_info();
            let fragments = backend.get_fragments_for_doc(&doc, &all_fragments);
            let used_fragments = backend.get_used_fragments();
            let _diagnostics = doc.get_semantic_diagnostics(
                &schema,
                &fragments,
                Some(&used_fragments),
                Some(&backend.config.read().unwrap()),
                false,
                true,
            );
        })
    });

    group.bench_function("Document Highlight", |b| {
        b.to_async(&rt).iter(|| {
            backend.document_highlight(DocumentHighlightParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(1, 10),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
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
