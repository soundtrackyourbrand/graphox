use criterion::{Criterion, criterion_group, criterion_main};
use graphox::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    document::DocumentState,
    engine,
};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{LanguageServer, LspService};

fn generate_call_hierarchy_workspace(
    base_dir: &Path,
    projects_count: usize,
    chain_depth: usize,
) -> Config {
    let mut projects = Vec::new();

    for i in 0..projects_count {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        let schema_path = project_dir.join("schema.graphql");
        let mut schema_content = String::new();

        schema_content.push_str("type User {\n  id: ID!\n  name: String!\n  email: String!\n}\n");
        schema_content.push_str("type Query {\n  getUser: User\n}\n");
        schema_content.push_str("schema { query: Query }\n");

        fs::write(&schema_path, schema_content).unwrap();

        for j in 0..5 {
            let file_path = project_dir.join(format!("file_{}.graphql", j));

            let mut content = String::new();

            if j < chain_depth {
                let next_in_chain = if j + 1 < chain_depth { j + 1 } else { 0 };

                content.push_str(&format!("query Chain_{}_{} {{\n", i, j));
                content.push_str("  getUser {\n");

                for d in 0..=j {
                    if d == j {
                        content.push_str(&format!(
                            "    ...Fragment_{}_{}_{}\n",
                            i,
                            next_in_chain,
                            d % 3
                        ));
                    } else {
                        content.push_str(&format!("    ...Fragment_{}_{}_{}\n", i, d, d % 3));
                    }
                }

                content.push_str("  }\n");
                content.push_str("}\n");
            }

            for f in 0..3 {
                let prev_in_chain = if f == 0 { chain_depth - 1 } else { f - 1 };
                content.push_str(&format!(
                    "fragment Fragment_{}_{}_{} on User {{ ...Fragment_{}_{}_{} }}\n",
                    i,
                    j,
                    f,
                    i,
                    prev_in_chain,
                    (f + 2) % 3
                ));
            }

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

fn bench_call_hierarchy(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let config = generate_call_hierarchy_workspace(&base_dir, 10, 5);

    let _guard = rt.enter();
    let (service, _) = LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(Backend::new(client, config.clone()))
    });
    let backend = service.inner();

    rt.block_on(async {
        let workspace_metadata =
            engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
        for project_meta in workspace_metadata.projects {
            for file_path in project_meta.files {
                let abs_path = fs::canonicalize(&file_path).unwrap();
                let uri = Uri::from_file_path(&abs_path).unwrap();
                let content = fs::read_to_string(&file_path).unwrap();
                let doc = DocumentState::new_from_thread_local(
                    uri.clone(),
                    &content,
                    PositionEncodingKind::UTF8,
                );
                backend.documents.insert(uri, std::sync::Arc::new(doc));
            }
        }
    });

    let target_uri = Uri::from_file_path(base_dir.join("project_0/file_0.graphql")).unwrap();
    let target_position = Position::new(2, 10);

    let mut group = c.benchmark_group("Call Hierarchy (10 projects, 5 deep chain)");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));

    group.bench_function("Prepare Call Hierarchy", |b| {
        b.to_async(&rt).iter(|| {
            backend.prepare_call_hierarchy(CallHierarchyPrepareParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: target_position,
                },
                work_done_progress_params: Default::default(),
            })
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(800))
        .measurement_time(Duration::from_millis(1000));
    targets = bench_call_hierarchy
);
criterion_main!(benches);
