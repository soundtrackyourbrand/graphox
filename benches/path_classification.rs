use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use graphox::{
    Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    utils,
};
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tower_lsp::lsp_types::Url;

const HOT_SET_SIZES: &[usize] = &[1, 32, 256];
const PROJECT_COUNT: usize = 8;
const FILES_PER_PROJECT: usize = 64;

struct PathBenchWorkspace {
    _temp_dir: TempDir,
    config: Config,
    source_paths: Vec<PathBuf>,
    output_paths: Vec<PathBuf>,
    source_uris: Vec<Url>,
}

impl PathBenchWorkspace {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let base_dir = temp_dir.path();

        let mut projects = Vec::new();
        let mut source_paths = Vec::new();
        let mut output_paths = Vec::new();
        let mut source_uris = Vec::new();

        for project_idx in 0..PROJECT_COUNT {
            let project_dir = base_dir.join(format!("project_{project_idx}"));
            let source_dir = project_dir.join("src");
            let output_dir = source_dir.join("__generated__");
            fs::create_dir_all(&output_dir).expect("create output dir");

            let schema_path = project_dir.join("schema.graphql");
            fs::write(
                &schema_path,
                "type Query { viewer: User } type User { id: ID! name: String }",
            )
            .expect("write schema");

            for file_idx in 0..FILES_PER_PROJECT {
                let source_path = source_dir.join(format!("query_{file_idx}.ts"));
                fs::write(
                    &source_path,
                    format!(
                        "export const query{file_idx} = graphql(`query Query{file_idx} {{ viewer {{ id name }} }}`);\n"
                    ),
                )
                .expect("write source file");
                source_uris.push(Url::from_file_path(&source_path).expect("source uri"));
                source_paths.push(source_path);

                let output_path = output_dir.join(format!("query_{file_idx}.codegen.ts"));
                fs::write(
                    &output_path,
                    format!("export type Query{file_idx} = {{ __typename: 'Query' }};\n"),
                )
                .expect("write output file");
                output_paths.push(output_path);
            }

            projects.push(
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single(format!(
                        "project_{project_idx}/schema.graphql"
                    )))
                    .with_include(GlobPattern::Single(format!(
                        "project_{project_idx}/src/**/*.{{ts,tsx}}"
                    )))
                    .with_output_dir(format!("project_{project_idx}/src/__generated__")),
            );
        }

        let config = Config::new_empty()
            .with_projects(projects)
            .with_base_dir(base_dir.to_path_buf());

        Self {
            _temp_dir: temp_dir,
            config,
            source_paths,
            output_paths,
            source_uris,
        }
    }
}

fn bench_path_set<T>(b: &mut criterion::Bencher<'_>, items: &[T], mut op: impl FnMut(&T)) {
    let mut idx = 0usize;
    b.iter(|| {
        let item = &items[idx % items.len()];
        idx = idx.wrapping_add(1);
        op(item);
    });
}

fn bench_is_output_file(c: &mut Criterion) {
    let workspace = PathBenchWorkspace::new();
    let mut group = c.benchmark_group("Path Classification / is_output_file");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_millis(1500));

    for &hot_set_size in HOT_SET_SIZES {
        let output_paths = &workspace.output_paths[..hot_set_size];
        let source_paths = &workspace.source_paths[..hot_set_size];

        for path in output_paths.iter().chain(source_paths.iter()) {
            let _ = workspace.config.is_output_file(path);
        }

        group.bench_with_input(
            BenchmarkId::new("generated files", hot_set_size),
            &hot_set_size,
            |b, _| {
                bench_path_set(b, output_paths, |path| {
                    black_box(workspace.config.is_output_file(black_box(path)));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("source files", hot_set_size),
            &hot_set_size,
            |b, _| {
                bench_path_set(b, source_paths, |path| {
                    black_box(workspace.config.is_output_file(black_box(path)));
                });
            },
        );
    }

    group.finish();
}

fn bench_get_project_for_path(c: &mut Criterion) {
    let workspace = PathBenchWorkspace::new();
    let mut group = c.benchmark_group("Path Classification / get_project_for_path");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_millis(1500));

    for &hot_set_size in HOT_SET_SIZES {
        let source_paths = &workspace.source_paths[..hot_set_size];
        for path in source_paths {
            let _ = workspace.config.get_project_for_path(path);
        }

        group.bench_with_input(
            BenchmarkId::new("source files", hot_set_size),
            &hot_set_size,
            |b, _| {
                bench_path_set(b, source_paths, |path| {
                    black_box(workspace.config.get_project_for_path(black_box(path)));
                });
            },
        );
    }

    group.finish();
}

fn bench_normalize_uri(c: &mut Criterion) {
    let workspace = PathBenchWorkspace::new();
    let mut group = c.benchmark_group("Path Classification / normalize_uri");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_millis(1500));

    for &hot_set_size in HOT_SET_SIZES {
        let source_uris = &workspace.source_uris[..hot_set_size];
        for uri in source_uris {
            let _ = utils::normalize_uri(uri.clone());
        }

        group.bench_with_input(
            BenchmarkId::new("source uris", hot_set_size),
            &hot_set_size,
            |b, _| {
                bench_path_set(b, source_uris, |uri| {
                    black_box(utils::normalize_uri(black_box(uri.clone())));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(300))
        .measurement_time(Duration::from_millis(1500));
    targets = bench_is_output_file, bench_get_project_for_path, bench_normalize_uri
);
criterion_main!(benches);
