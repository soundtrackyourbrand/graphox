//! LSP cold-start benchmarks.
//!
//! `graphox check` and the LSP do roughly the same work, yet opening a file from
//! scratch in a large JS monorepo feels far slower than `check` (~0.3s). The cause
//! is LSP-only work that runs *synchronously in `Backend::new`* and blocks the
//! `initialize` response before any diagnostics can flow.
//!
//! These benchmarks isolate the hot paths on that critical path so they can be
//! tuned and protected against regressions:
//!
//! 1. `Gitignore Matcher` — `get_gitignore_matcher` walks the workspace to collect
//!    `.gitignore` files. If the walk does not honour `.gitignore` it descends into
//!    `node_modules` (hundreds of thousands of files on a real monorepo). The
//!    `pruned` vs `unpruned` cases below quantify that gap directly.
//! 2. `Backend Startup` — the full synchronous `Backend::new`, i.e. everything that
//!    delays the `initialize` response (schema load + validate + gitignore matcher).
//! 3. `Fragment Metadata` — `collect_fragment_metadata`, rebuilt on every
//!    did_open / did_change / pull-diagnostic, so it sits on the per-edit hot path.
//!
//! The generators create a `node_modules` blowup (ignored files) alongside a small
//! set of real project files, mirroring the shape of a TypeScript monorepo.

use criterion::{Criterion, criterion_group, criterion_main};
use graphox::DocumentState;
use graphox::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphox::Config;
use graphox_lsp::backend::fragment_manager::collect_fragment_metadata;
use graphox_lsp::backend::state::Backend;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::lsp_types::{PositionEncodingKind, Url};

/// Build a synthetic monorepo: a handful of real `.graphql` project files plus a
/// large `node_modules` tree of ignored files, with the nested `.gitignore` files
/// a real repo would contain.
///
/// `node_modules_files` is the number of ignored files to scatter under
/// `node_modules`; real monorepos reach hundreds of thousands.
fn generate_monorepo(base: &Path, project_files: usize, node_modules_files: usize) {
    // The `ignore` crate only applies `.gitignore` rules inside a recognised git
    // repo (its `require_git` defaults to true). A bare `.git` directory is enough
    // for it to treat `base` as the repo root — without this, `git_ignore(true)`
    // would not prune `node_modules` and the bench would not reflect a real repo.
    fs::create_dir_all(base.join(".git")).unwrap();
    fs::write(base.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();

    // Root .gitignore that prunes the heavy directories, like a real repo.
    fs::write(base.join(".gitignore"), "node_modules\ndist\ntarget\n").unwrap();

    // Real schema + project files.
    let mut schema = String::from("type Query { user: User }\n");
    schema.push_str("type User { id: ID! name: String email: String }\n");
    fs::write(base.join("schema.graphql"), schema).unwrap();

    let src = base.join("src");
    fs::create_dir_all(&src).unwrap();
    for i in 0..project_files {
        let content = format!(
            "query GetUser{i} {{ user {{ ...UserFields }} }}\n\
             fragment UserFields{i} on User {{ id name email }}\n"
        );
        fs::write(src.join(format!("file_{i}.graphql")), content).unwrap();
    }

    // node_modules blowup: many small files plus the occasional nested .gitignore,
    // spread across a directory fan-out so the walk has real tree structure.
    let nm = base.join("node_modules");
    let dirs = 64usize;
    let per_dir = node_modules_files.div_ceil(dirs);
    for d in 0..dirs {
        let pkg = nm.join(format!("dep_{d}")).join("src");
        fs::create_dir_all(&pkg).unwrap();
        // A nested .gitignore inside an ignored subtree — must NOT be walked.
        fs::write(nm.join(format!("dep_{d}")).join(".gitignore"), "*.log\n").unwrap();
        for f in 0..per_dir {
            fs::write(pkg.join(format!("mod_{f}.js")), "module.exports = {};\n").unwrap();
        }
    }
}

/// Replica of the *old* `get_gitignore_matcher` behaviour: walk without honouring
/// `.gitignore`, so the walk descends into `node_modules`. Kept here purely as a
/// benchmark baseline to quantify the cost of the pruning fix.
fn gitignore_matcher_unpruned(base_dir: &Path) -> ignore::gitignore::Gitignore {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(base_dir);
    for entry in ignore::WalkBuilder::new(base_dir)
        .hidden(false)
        .git_ignore(false)
        .build()
    {
        if let Ok(entry) = entry
            && entry.file_name() == ".gitignore"
        {
            let _ = builder.add(entry.path());
        }
    }
    builder
        .build()
        .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
}

fn bench_gitignore_matcher(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    // ~16k ignored files. A real monorepo is ~30x larger; this is enough to make
    // the pruned/unpruned gap obvious while keeping setup fast.
    generate_monorepo(&base, 50, 16_000);

    let mut group = c.benchmark_group("Gitignore Matcher");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(2000));

    // Current behaviour: walk honours .gitignore, so node_modules is pruned.
    group.bench_function("pruned (git_ignore=true)", |b| {
        b.iter(|| graphox::utils::get_gitignore_matcher(&base))
    });

    // Old behaviour: walk descends into node_modules.
    group.bench_function("unpruned (git_ignore=false)", |b| {
        b.iter(|| gitignore_matcher_unpruned(&base))
    });

    group.finish();
}

fn bench_backend_startup(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    generate_monorepo(&base, 50, 16_000);

    let config = Config::new_empty()
        .with_projects(vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("src/**/*.graphql".to_string())),
        ])
        .with_base_dir(base.clone())
        .with_enable_schema_cache(true);

    let _guard = rt.enter();

    let mut group = c.benchmark_group("Backend Startup");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(2000));

    // Everything that blocks the `initialize` response: schema load + validate +
    // gitignore matcher build.
    group.bench_function("Backend::new (blocks initialize)", |b| {
        b.iter(|| {
            let (service, _) =
                LspService::new(|client| Backend::new(client, config.clone()));
            // Keep the constructed backend alive across the measurement.
            std::hint::black_box(service.inner().documents.len());
        })
    });

    group.finish();
}

/// Seed a backend's maps with `files` documents, each defining `frags_per_file`
/// fragments, so `collect_fragment_metadata` has realistic work to do.
fn seed_backend(base: &Path, files: usize, frags_per_file: usize) -> Arc<Backend> {
    fs::write(
        base.join("schema.graphql"),
        "type Query { user: User } type User { id: ID! name: String email: String }",
    )
    .unwrap();

    let config = Config::new_test(
        base.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    );

    let (service, _) = LspService::new(|client| Backend::new(client, config));
    let backend = service.inner().clone();

    for i in 0..files {
        let uri = Url::from_file_path(base.join(format!("doc_{i}.graphql"))).unwrap();
        let mut content = format!("query GetUser{i} {{ user {{ ...Frag{i}_0 }} }}\n");
        for f in 0..frags_per_file {
            content.push_str(&format!("fragment Frag{i}_{f} on User {{ id name email }}\n"));
        }
        let doc =
            DocumentState::new_from_thread_local(uri.clone(), &content, PositionEncodingKind::UTF16);

        let metadata = Arc::new(graphox::types::DocumentMetadata {
            fragments: doc.fragments.clone(),
            fragment_spreads: doc.fragment_spreads.clone(),
            package_root: doc.package_root.clone(),
            operations: doc.operations.clone(),
            version: 0,
        });
        backend.documents.insert(uri.clone(), Arc::new(doc));
        backend.metadata.insert(uri, metadata);
    }

    backend
}

fn bench_fragment_metadata(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let mut group = c.benchmark_group("Fragment Metadata");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(2000));

    for (files, frags) in [(100usize, 5usize), (500, 5), (500, 20)] {
        let dir = tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let backend = seed_backend(&base, files, frags);
        let config = backend.config.read().unwrap().clone();

        group.bench_function(
            format!("collect_fragment_metadata ({files} files x {frags} frags)"),
            |b| {
                b.iter(|| {
                    collect_fragment_metadata(
                        &backend.metadata,
                        &config,
                        &backend.subgraphs,
                        &backend.documents,
                        &backend.schemas,
                    )
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_millis(2000));
    targets = bench_gitignore_matcher, bench_backend_startup, bench_fragment_metadata
);
criterion_main!(benches);
