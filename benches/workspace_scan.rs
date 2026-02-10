use criterion::{Criterion, criterion_group, criterion_main};
use graphox::{
    Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    engine,
};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;
use tower_lsp::lsp_types::PositionEncodingKind;

fn generate_workspace_with_schemas(
    base_dir: &Path,
    projects_count: usize,
    files_per_project: usize,
    type_count: usize,
    fields_per_type: usize,
) -> Config {
    let mut projects = Vec::new();

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

            let query = format!("query Q{} {{\n  getType0 {{ field_0 }}\n}}", j);

            let fragments: String = (0..2)
                .map(|f| {
                    format!(
                        "\nfragment Frag{}_{}_{} on Type{} {{ field_{} }}",
                        i,
                        j,
                        f,
                        f % type_count,
                        f % fields_per_type
                    )
                })
                .collect::<String>();

            let content = format!("{}{}", query, fragments);
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

fn bench_workspace_scan(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let mut group = c.benchmark_group("Workspace Scan");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(Duration::from_millis(1000));

    group.bench_function("500 files, 10 projects, 50 types × 20 fields", |b| {
        b.iter(|| {
            let config = generate_workspace_with_schemas(&base_dir, 10, 50, 50, 20);
            rt.block_on(async {
                engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
            })
        })
    });

    group.bench_function("1000 files, 20 projects, 50 types × 20 fields", |b| {
        b.iter(|| {
            let config = generate_workspace_with_schemas(&base_dir, 20, 50, 50, 20);
            rt.block_on(async {
                engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
            })
        })
    });

    group.bench_function("2000 files, 20 projects, 100 types × 30 fields", |b| {
        b.iter(|| {
            let config = generate_workspace_with_schemas(&base_dir, 20, 100, 100, 30);
            rt.block_on(async {
                engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
            })
        })
    });

    group.bench_function("5000 files, 50 projects, 50 types × 20 fields", |b| {
        b.iter(|| {
            let config = generate_workspace_with_schemas(&base_dir, 50, 100, 50, 20);
            rt.block_on(async {
                engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
            })
        })
    });

    group.finish();
}

fn bench_workspace_scan_incremental(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let mut group = c.benchmark_group("Workspace Scan (Incremental)");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(Duration::from_millis(1000));

    let config = generate_workspace_with_schemas(&base_dir, 10, 50, 50, 20);
    let initial_metadata = rt.block_on(async {
        engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
    });

    group.bench_function("Rescan (500 files, no changes)", |b| {
        b.iter(|| {
            rt.block_on(async {
                engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, Some(&initial_metadata))
            })
        })
    });

    group.bench_function("Rescan (500 files, 10 files changed)", |b| {
        b.iter(|| {
            // Modify 10 files
            for j in 0..10 {
                let file_path = base_dir.join(format!("project_0/file_{}.graphql", j));
                let content = fs::read_to_string(&file_path).unwrap();
                let new_content = format!("{} ", content); // Add a space to change it
                fs::write(&file_path, new_content).unwrap();
            }

            rt.block_on(async {
                engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, Some(&initial_metadata))
            })
        })
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(800))
        .measurement_time(Duration::from_millis(1000));
    targets = bench_workspace_scan, bench_workspace_scan_incremental
);
criterion_main!(benches);
