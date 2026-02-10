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
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

fn generate_cross_project_workspace(base_dir: &Path, projects_count: usize, files_per_project: usize) -> Config {
    let mut projects = Vec::new();
    let type_count = 30;
    let fields_per_type = 15;

    for i in 0..projects_count {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        let schema_path = project_dir.join("schema.graphql");
        let mut schema_content = String::new();

        for t in 0..type_count {
            schema_content.push_str(&format!("type Type{} {{\n", t));
            for f in 0..fields_per_type {
                schema_content.push_str(&format!("  field_{}: String\n", f));
            }
            schema_content.push_str("}\n");
        }

        schema_content.push_str("type Query {\n");
        for t in 0..type_count {
            schema_content.push_str(&format!("  getType{}: Type{}\n", t, t));
        }
        schema_content.push_str("}\n");
        schema_content.push_str("schema { query: Query }\n");

        fs::write(&schema_path, schema_content).unwrap();

        for j in 0..files_per_project {
            let file_path = project_dir.join(format!("file_{}.graphql", j));
            let mut content = String::new();

            content.push_str(&format!("query Q{} {{\n", j));
            content.push_str("  getType0 { ");

            let spreads_per_file = 10;
            for s in 0..spreads_per_file {
                if i == 0 {
                    content.push_str(&format!("field_{} ", s));
                } else {
                    content.push_str(&format!("...BaseFrag{} ", s % spreads_per_file));
                }
            }
            content.push_str("}\n}\n");

            if i == 0 {
                for s in 0..spreads_per_file {
                    content.push_str(&format!(
                        "fragment BaseFrag{} on Type{} {{ field_{} }}\n",
                        s,
                        s % type_count,
                        s % fields_per_type
                    ));
                }
            }

            fs::write(file_path, content).unwrap();
        }

        projects.push(ProjectConfig {
            schema: SchemaSource::Single(format!("project_{}/schema.graphql", i)),
            include: GlobPattern::Single(format!("project_{}/**/*.graphql", i)),
            ..Default::default()
        });
    }

    Config {
        projects,
        base_dir: base_dir.to_path_buf(),
        enable_schema_cache: Some(true),
        ..Default::default()
    }
}

fn bench_cross_project_resolution(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let config = generate_cross_project_workspace(&base_dir, 10, 20);

    let _guard = rt.enter();
    let (service, _) = LspService::new(|client| Backend::new(client, config.clone()));
    let backend = service.inner();

    rt.block_on(async {
        let workspace_metadata = engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
        for project_meta in workspace_metadata.projects {
            for file_path in project_meta.files {
                let abs_path = fs::canonicalize(&file_path).unwrap();
                let uri = Url::from_file_path(&abs_path).unwrap();
                let content = fs::read_to_string(&file_path).unwrap();
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&tree_sitter_graphql::LANGUAGE.into()).unwrap();
                let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);
                backend.documents.insert(uri, std::sync::Arc::new(doc));
            }
        }
    });

    let target_uri = Url::from_file_path(base_dir.join("project_0/file_0.graphql")).unwrap();

    let mut group = c.benchmark_group("Cross-Project Resolution (10 projects, 200 files, 100 base fragments)");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));

    group.bench_function("Get All Fragments Info", |b| {
        b.to_async(&rt).iter(|| {
            async {
                backend.get_all_fragments_info()
            }
        });
    });

    group.bench_function("Resolve Fragments for Doc", |b| {
        b.to_async(&rt).iter(|| {
            async {
                let doc = backend.documents.get(&target_uri).map(|r| r.value().clone()).unwrap();
                let all_fragments = backend.get_all_fragments_info();
                backend.get_fragments_for_doc(&doc, &all_fragments)
            }
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
