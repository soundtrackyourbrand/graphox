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
use tower_lsp_server::ls_types::PositionEncodingKind;

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
                engine::Engine::scan_workspace(
                    &config,
                    PositionEncodingKind::UTF8,
                    Some(&initial_metadata),
                )
            })
        })
    });

    group.bench_function("Rescan (500 files, 10 files changed)", |b| {
        b.iter(|| {
            for j in 0..10 {
                let file_path = base_dir.join(format!("project_0/file_{}.graphql", j));
                let content = fs::read_to_string(&file_path).unwrap();
                let new_content = format!("{} ", content);
                fs::write(&file_path, new_content).unwrap();
            }

            rt.block_on(async {
                engine::Engine::scan_workspace(
                    &config,
                    PositionEncodingKind::UTF8,
                    Some(&initial_metadata),
                )
            })
        })
    });
}

// ============================================================================
// FRAGMENT-HEAVY BENCHMARKS - These trigger O(N²) behavior
// ============================================================================

fn generate_fragment_heavy_workspace(
    base_dir: &Path,
    file_count: usize,
    fragments_per_file: usize,
    schema_types: usize,
    schema_fields: usize,
) -> Config {
    let project_dir = base_dir.join("project_0");
    fs::create_dir_all(&project_dir).unwrap();

    let schema_path = project_dir.join("schema.graphql");
    let mut schema_content = String::new();

    for t in 0..schema_types {
        schema_content.push_str(&format!("type Type{} {{\n", t));
        for f in 0..schema_fields {
            schema_content.push_str(&format!("  field_{}: String\n", f));
        }
        schema_content.push_str("}\n");
    }

    schema_content.push_str("type Query {\n");
    for t in 0..schema_types {
        schema_content.push_str(&format!("  getType{}: Type{}\n", t, t));
    }
    schema_content.push_str("}\n");
    schema_content.push_str("schema { query: Query }\n");

    fs::write(&schema_path, schema_content).unwrap();

    for j in 0..file_count {
        let file_path = project_dir.join(format!("file_{}.graphql", j));

        let query = format!("query Q{} {{\n  getType0 {{ field_0 }}\n}}", j);

        let fragments: String = (0..fragments_per_file)
            .map(|f| {
                format!(
                    "\nfragment Frag{}_{} on Type{} {{ field_{} }}",
                    j,
                    f,
                    f % schema_types,
                    f % schema_fields
                )
            })
            .collect::<String>();

        let content = format!("{}{}", query, fragments);
        fs::write(file_path, content).unwrap();
    }

    let projects = vec![
        ProjectConfig::default()
            .with_schema(SchemaSource::Single("project_0/schema.graphql".to_string()))
            .with_include(GlobPattern::Single("project_0/**/*.graphql".to_string())),
    ];

    Config::new_empty()
        .with_projects(projects)
        .with_base_dir(base_dir.to_path_buf())
        .with_enable_schema_cache(true)
}

fn bench_fragment_heavy(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let mut group = c.benchmark_group("Fragment-Heavy");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(5000));

    group.bench_function("50 files, 10 frags/file (500 total)", |b| {
        b.iter(|| {
            let config = generate_fragment_heavy_workspace(&base_dir, 50, 10, 20, 10);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let _ = deps_ms;
                result.fragments.len();
                result
            })
        })
    });

    group.bench_function("100 files, 20 frags/file (2000 total)", |b| {
        b.iter(|| {
            let config = generate_fragment_heavy_workspace(&base_dir, 100, 20, 30, 15);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let _ = deps_ms;
                result.fragments.len();
                result
            })
        })
    });

    group.bench_function("200 files, 50 frags/file (10000 total)", |b| {
        b.iter(|| {
            let config = generate_fragment_heavy_workspace(&base_dir, 200, 50, 50, 20);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let _ = deps_ms;
                result.fragments.len();
                result
            })
        })
    });

    group.finish();
}

// ============================================================================
// CROSS-FRAGMENT REFERENCES - Fragments that reference each other
// ============================================================================

fn generate_cross_referencing_workspace(
    base_dir: &Path,
    fragment_count: usize,
    refs_per_fragment: usize,
    schema_types: usize,
) -> Config {
    let project_dir = base_dir.join("project_0");
    fs::create_dir_all(&project_dir).unwrap();

    let schema_path = project_dir.join("schema.graphql");
    let mut schema_content = String::new();

    for t in 0..schema_types {
        schema_content.push_str(&format!("type Type{} {{\n", t));
        schema_content.push_str("  id: ID!\n");
        schema_content.push_str("}\n");
    }
    schema_content.push_str("type Query {\n");
    schema_content.push_str("  root: Type0\n");
    schema_content.push_str("}\n");
    schema_content.push_str("schema { query: Query }\n");

    fs::write(&schema_path, schema_content).unwrap();

    for i in 0..fragment_count {
        let file_path = project_dir.join(format!("frag_{}.graphql", i));

        let refs: String = (0..refs_per_fragment)
            .map(|r| {
                let target = (i + r + 1) % fragment_count;
                format!("...Frag{}\n", target)
            })
            .collect::<String>();

        let content = format!(
            "fragment Frag{} on Type{} {{\n  id\n{}\n}}\n",
            i,
            i % schema_types,
            refs
        );
        fs::write(file_path, content).unwrap();
    }

    let query_path = project_dir.join("queries.graphql");
    let query_content = format!(
        "query Test {{\n  root {{\n    id\n{}\n  }}\n}}\n",
        (0..10.min(fragment_count))
            .map(|i| format!("    ...Frag{}", i))
            .collect::<String>()
    );
    fs::write(query_path, query_content).unwrap();

    let projects = vec![
        ProjectConfig::default()
            .with_schema(SchemaSource::Single("project_0/schema.graphql".to_string()))
            .with_include(GlobPattern::Single("project_0/**/*.graphql".to_string())),
    ];

    Config::new_empty()
        .with_projects(projects)
        .with_base_dir(base_dir.to_path_buf())
        .with_enable_schema_cache(true)
}

fn bench_cross_references(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let mut group = c.benchmark_group("Cross-References");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(5000));

    group.bench_function("100 frags, 5 refs each", |b| {
        b.iter(|| {
            let config = generate_cross_referencing_workspace(&base_dir, 100, 5, 50);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let _ = deps_ms;
                result.fragments.len();
                result
            })
        })
    });

    group.bench_function("500 frags, 10 refs each", |b| {
        b.iter(|| {
            let config = generate_cross_referencing_workspace(&base_dir, 500, 10, 100);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let _ = deps_ms;
                result.fragments.len();
                result
            })
        })
    });

    group.finish();
}

// ============================================================================
// PHASE TIMING BENCHMARKS - Break down where time is spent
// ============================================================================

fn bench_phase_timings(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let mut group = c.benchmark_group("Phase Timings");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(Duration::from_millis(2000));

    group.bench_function("200 files, medium complexity", |b| {
        b.iter(|| {
            let config = generate_workspace_with_schemas(&base_dir, 5, 40, 30, 15);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let glob_ms = result.timings.glob_resolution.as_millis();
                let parse_ms = result.timings.doc_parsing.as_millis();
                let metadata_ms = result.timings.metadata_extraction.as_millis();
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let total = glob_ms + parse_ms + metadata_ms + deps_ms;
                let _ = (
                    glob_ms,
                    parse_ms,
                    metadata_ms,
                    deps_ms,
                    total,
                    result.fragments.len(),
                );
                result
            })
        })
    });

    group.bench_function("200 files, 50 frags/file (O(N²) test)", |b| {
        b.iter(|| {
            let config = generate_fragment_heavy_workspace(&base_dir, 200, 50, 50, 20);
            rt.block_on(async {
                let result =
                    engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
                let glob_ms = result.timings.glob_resolution.as_millis();
                let parse_ms = result.timings.doc_parsing.as_millis();
                let metadata_ms = result.timings.metadata_extraction.as_millis();
                let deps_ms = result.timings.fragment_deps_computation.as_millis();
                let total = glob_ms + parse_ms + metadata_ms + deps_ms;
                let _ = (
                    glob_ms,
                    parse_ms,
                    metadata_ms,
                    deps_ms,
                    total,
                    result.fragments.len(),
                );
                result
            })
        })
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(800))
        .measurement_time(Duration::from_millis(1000));
    targets =
        bench_workspace_scan,
        bench_workspace_scan_incremental,
        bench_fragment_heavy,
        bench_cross_references,
        bench_phase_timings
);
criterion_main!(benches);
