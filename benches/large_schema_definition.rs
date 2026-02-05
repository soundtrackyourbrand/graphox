use criterion::{Criterion, criterion_group, criterion_main};
use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    document::DocumentState,
};
use std::fs;
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

fn generate_complex_workspace(base_dir: &std::path::Path, project_count: usize, fields_per_schema: usize, files_per_project: usize) -> Config {
    let mut projects = Vec::new();

    for i in 0..project_count {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        // Large schema for each project
        let schema_path = project_dir.join("schema.graphql");
        let mut schema_content = format!("type Query_{} {{\n", i);
        for j in 0..fields_per_schema {
            schema_content.push_str(&format!("  field_{}: String\n", j));
        }
        schema_content.push_str("}\n");
        schema_content.push_str(&format!("schema {{ query: Query_{} }}\n", i));
        fs::write(&schema_path, schema_content).unwrap();

        // Many small files (operations/fragments)
        for j in 0..files_per_project {
            let file_path = project_dir.join(format!("file_{}.graphql", j));
            let content = format!(
                "query GetField{}_{} {{ field_{} }}\nfragment Frag{}_{} on Query_{} {{ field_0 }}",
                i, j, fields_per_schema - 1, i, j, i
            );
            fs::write(file_path, content).unwrap();
        }

        projects.push(ProjectConfig {
            schema: SchemaSource::Single(format!("project_{}/schema.graphql", i)),
            include: GlobPattern::Multiple(vec![format!("project_{}/**/*.graphql", i)]),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        });
    }

    Config {
        projects,
        base_dir: base_dir.to_path_buf(),
        ..Config::new_empty()
    }
}

fn bench_complex_workspace_definition(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // 10 projects, 10k fields each, 200 files each = 2000 files total + 10 schemas
    let project_count = 10;
    let fields_per_schema = 10000;
    let files_per_project = 200;
    
    let config = generate_complex_workspace(&base_dir, project_count, fields_per_schema, files_per_project);

    let (service, _) = LspService::new(|client| Backend::new(client, config.clone()));
    let backend = service.inner();

    // Pre-populate ALL documents (schemas are already loaded in Backend::new)
    rt.block_on(async {
        for i in 0..project_count {
            for j in 0..files_per_project {
                let path = base_dir.join(format!("project_{}/file_{}.graphql", i, j));
                let uri = Url::from_file_path(fs::canonicalize(&path).unwrap()).unwrap();
                let content = fs::read_to_string(&path).unwrap();
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&tree_sitter_graphql::LANGUAGE.into()).unwrap();
                let doc = DocumentState::new(uri.clone(), &content, parser);
                backend.documents.insert(uri, std::sync::Arc::new(doc));
            }
        }
    });

    // Target a field in the FIRST project to see if it's faster
    let target_project = 0;
    let target_uri = Url::from_file_path(base_dir.join(format!("project_{}/file_0.graphql", target_project))).unwrap();

    let mut group = c.benchmark_group("Complex Workspace Definition");
    group.sample_size(10);

    group.bench_function("Definition: First Project Field in 2000 docs", |b| {

        b.to_async(&rt).iter(|| {
            backend.goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, 10), // on field_9999
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.finish();
}

criterion_group!(benches, bench_complex_workspace_definition);
criterion_main!(benches);
