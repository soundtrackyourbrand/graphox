use criterion::{Criterion, criterion_group, criterion_main};
use graphox_core::Config;
use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphox_lsp::backend::state::Backend;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tower_lsp_server::ls_types::*;

pub fn bench_cross_project_resolution(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create multiple projects to exercise cross-project dependency resolution and performance characteristics
    let mut projects = Vec::new();
    for i in 0..10 {
        let proj_dir = base_dir.join(format!("project_{}", i));
        std::fs::create_dir_all(&proj_dir).unwrap();

        let schema_file = format!("project_{}/schema.graphql", i);
        std::fs::write(
            base_dir.join(&schema_file),
            format!(
                "type Query {{ user{}: User{} }} type User{} {{ id: ID! name: String }}",
                i, i, i
            ),
        )
        .unwrap();

        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Single(schema_file))
                .with_include(GlobPattern::Single(format!("project_{}/**/*.graphql", i))),
        );
    }

    let config = Config::new_test(base_dir.clone(), projects);
    let _guard = rt.enter();
    let (service, _) = tower_lsp_server::LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(Backend::new(client, config.clone()))
    });
    let backend = service.inner();

    // Seed multiple files per project with fragment definitions and spreads to exercise metadata collection
    rt.block_on(async {
        for i in 0..10 {
            for j in 0..20 {
                let path = base_dir.join(format!("project_{}/file_{}.graphql", i, j));
                let content = format!(
                    "fragment UserFields{}_{} on User{} {{ id name }} query GetUser{}_{} {{ user{} {{ ...UserFields{}_{} }} }}",
                    i, j, i, i, j, i, i, j
                );
                std::fs::write(&path, &content).unwrap();

                let uri = graphox::utils::path_to_uri(&path).unwrap();
                let doc = graphox_core::DocumentState::new_from_thread_local(
                    uri.clone(),
                    &content,
                    PositionEncodingKind::UTF16,
                );
                backend
                    .documents
                    .insert(uri.clone(), std::sync::Arc::new(doc));

                let metadata = Arc::new(graphox_core::types::DocumentMetadata {
                    fragments: Arc::from([graphox_core::document::FragmentDef {
                        name: format!("UserFields{}_{}", i, j).into(),
                        type_condition: format!("User{}", i).into(),
                        is_public: true,
                        is_type_only: false,
                        description: None,
                        source_hash: 0,
                        used_variables: Arc::from([]),
                        used_fragments: Arc::from([]),
                        transitive_deps: Arc::from([]),
                        selected_fields: Arc::from([]),
                        type_fields: Arc::from([]),
                        top_level_spreads: Arc::from([]),
                        nested_selections: Arc::from([]),
                        selection_ignores: Arc::from([]),
                        spread_ignores: Arc::from([]),
                    }]),
                    fragment_spreads: Arc::from([format!("UserFields{}_{}", i, j).into()]),
                    package_root: None,
                    operations: Arc::from([graphox_core::document::OperationDef {
                        name: Some(format!("GetUser{}_{}", i, j).into()),
                        operation_type: "query".into(),
                        source_text: content.into(),
                    }]),
                    version: 0,
                });
                backend.metadata.insert(uri, metadata);
            }
        }
    });

    let target_uri =
        graphox::utils::path_to_uri(base_dir.join("project_0/file_0.graphql")).unwrap();

    let mut group =
        c.benchmark_group("Cross-Project Resolution (10 projects, 200 files, 100 base fragments)");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));

    group.bench_function("Collect Fragment Metadata", |b| {
        b.to_async(&rt)
            .iter(|| async { backend.get_all_fragments_info() });
    });

    group.bench_function("Resolve Fragments for Doc", |b| {
        b.to_async(&rt).iter(|| async {
            let doc = backend
                .documents
                .get(&target_uri)
                .map(|r| r.value().clone())
                .unwrap();
            let all_fragments = backend.get_all_fragments_info();
            backend.get_fragments_for_doc(&doc, &all_fragments)
        });
    });

    group.bench_function("Codegen with Cross-Project Resolution", |b| {
        b.to_async(&rt).iter(|| backend.run_codegen());
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_millis(800)).measurement_time(Duration::from_millis(1000));
    targets = bench_cross_project_resolution
);
criterion_main!(benches);
