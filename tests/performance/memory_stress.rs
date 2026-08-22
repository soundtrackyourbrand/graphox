//! Memory stress tests.
//! These tests verify memory usage under various stress conditions.

use crate::support::create_large_schema;
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// Upper bounds on the live heap each scenario is allowed to retain. The live heap
// (allocated minus freed) is measured deterministically by the tracking allocator
// installed in `performance_suite.rs`, unlike process RSS which is polluted by
// allocator pool retention and run-order effects. Because every heap-measuring /
// heap-retaining perf test serializes on `PERF_MEMORY_MUTEX`, these deltas are
// stable run-to-run, so each limit is set just above the measured value (noted
// inline) — tight enough to catch a real retention/leak regression.
const MAX_HEAP_OPEN_CLOSE_CYCLES: usize = 4 * 1024 * 1024; // measures ~0 MB
const MAX_HEAP_CACHED_100_DOCS: usize = 6 * 1024 * 1024; // measures ~2.6 MB
const MAX_HEAP_10_SCHEMAS: usize = 4 * 1024 * 1024; // measures ~0.1 MB
const MAX_HEAP_500_FRAGMENTS: usize = 4 * 1024 * 1024; // measures ~1.5 MB
const MAX_HEAP_LARGE_SCHEMA: usize = 6 * 1024 * 1024; // measures ~3 MB
const MAX_MEMORY_COMPLEX_MONOREPO: usize = 80 * 1024 * 1024; // measures ~68 MB

use crate::support::PERF_MEMORY_MUTEX;

fn create_multi_project_config(base_dir: &Path) -> Config {
    let mut projects = Vec::new();

    // Projects using Schema A only (3 projects)
    for i in 0..3 {
        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schemas/schema_a.graphql".to_string()))
                .with_include(GlobPattern::Multiple(vec![
                    format!("project_{}/**/*.graphql", i),
                    format!("project_{}/**/*.ts", i),
                    format!("project_{}/**/*.tsx", i),
                ]))
                .with_codegen(CodegenConfig::disabled()),
        );
    }

    // Projects using Schema B only (3 projects)
    for i in 3..6 {
        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schemas/schema_b.graphql".to_string()))
                .with_include(GlobPattern::Multiple(vec![
                    format!("project_{}/**/*.graphql", i),
                    format!("project_{}/**/*.ts", i),
                    format!("project_{}/**/*.tsx", i),
                ]))
                .with_codegen(CodegenConfig::disabled()),
        );
    }

    // Projects using both schemas (4 projects)
    for i in 6..10 {
        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Multiple(vec![
                    "schemas/schema_a.graphql".to_string(),
                    "schemas/schema_b_extension.graphql".to_string(),
                ]))
                .with_include(GlobPattern::Multiple(vec![
                    format!("project_{}/**/*.graphql", i),
                    format!("project_{}/**/*.ts", i),
                    format!("project_{}/**/*.tsx", i),
                ]))
                .with_codegen(CodegenConfig::disabled()),
        );
    }

    Config::new_test(base_dir.to_owned(), projects)
        .with_enable_schema_cache(true)
        .with_lsp_automatic_codegen(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(18000)]
async fn test_memory_complex_monorepo_workspace_scan() {
    let _lock = PERF_MEMORY_MUTEX.lock().await;
    let baseline = crate::support::measure_allocated_bytes();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    // 1. Create 2 complex schemas (1000 types each)
    fs::create_dir_all(base_dir.join("schemas")).unwrap();
    let schema_a = crate::support::create_complex_schema_a(1000);
    let schema_b = crate::support::create_complex_schema_b(1000);
    let schema_b_extension = schema_b
        .lines()
        .filter(|line| !line.starts_with("directive @key("))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("type Query {", "extend type Query {")
        .replace("type Mutation {", "extend type Mutation {")
        .replace("type Subscription {", "extend type Subscription {");
    fs::write(base_dir.join("schemas/schema_a.graphql"), schema_a).unwrap();
    fs::write(base_dir.join("schemas/schema_b.graphql"), schema_b).unwrap();
    fs::write(
        base_dir.join("schemas/schema_b_extension.graphql"),
        schema_b_extension,
    )
    .unwrap();

    // 2. Create 10 projects
    for i in 0..10 {
        let schema_type = match i {
            0..=2 => "A",
            3..=5 => "B",
            _ => "both",
        };
        let project_dir = base_dir.join(format!("project_{}", i));
        crate::support::create_project_with_fragments(&project_dir, schema_type, i, 10);
    }

    // 3. Create multi-project config
    let config = create_multi_project_config(&base_dir);

    // 4. Initialize LSP service
    let (mut service, _) = tower_lsp_server::LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(graphox::Backend::new(client, config))
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    // Wait for workspace scan to complete (lsp_initialize_sequence already does this)
    // plus a short settle so transient scan buffers are freed before measuring.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 5. Measure the heap retained by the scanned workspace.
    let used = crate::support::measure_allocated_bytes();
    let delta = used.saturating_sub(baseline);

    println!(
        "Live heap for complex monorepo (10 projects, 2000+ types): {} MB",
        delta / 1024 / 1024
    );

    assert!(
        delta < MAX_MEMORY_COMPLEX_MONOREPO,
        "Memory exceeded limit for complex monorepo: {} MB (limit: {} MB)",
        delta / 1024 / 1024,
        MAX_MEMORY_COMPLEX_MONOREPO / 1024 / 1024
    );
}

fn create_100_file_config(base_dir: &Path) -> Config {
    Config::new_test(
        base_dir.to_owned(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false)
}

fn create_10_schema_config(base_dir: &Path) -> Config {
    Config::new_test(
        base_dir.to_owned(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_open_close_cycles() {
    let _lock = PERF_MEMORY_MUTEX.lock().await;
    let baseline = crate::support::measure_allocated_bytes();

    for _cycle in 0..10 {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let schema = create_large_schema(100);
        fs::write(base_dir.join("schema.graphql"), schema).unwrap();

        for i in 0..20 {
            let query = format!("query Query{} {{ item{} {{ id }} }}", i, i % 100);
            fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
        }

        let config = create_100_file_config(&base_dir);
        let (mut service, _) = tower_lsp_server::LspService::new(|client| {
            graphox::GraphoxLanguageServer::new(graphox::Backend::new(client, config))
        });
        crate::support::lsp_initialize_sequence(&mut service).await;

        for i in 0..20 {
            let uri = tower_lsp_server::ls_types::Uri::from_file_path(
                base_dir.join(format!("query_{}.graphql", i)),
            )
            .unwrap();
            let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
            crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let used = crate::support::measure_allocated_bytes();
    let delta = used.saturating_sub(baseline);

    println!(
        "Live heap after 10 open/close cycles (100 files each): {} KB",
        delta / 1024
    );
    assert!(
        delta < MAX_HEAP_OPEN_CLOSE_CYCLES,
        "Live heap exceeded limit for 10 open/close cycles: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_HEAP_OPEN_CLOSE_CYCLES / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_memory_cached_documents_100() {
    let _lock = PERF_MEMORY_MUTEX.lock().await;
    let baseline = crate::support::measure_allocated_bytes();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..100 {
        let query = format!("query Query{} {{ item{} {{ id name }} }}", i, i % 100);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = tower_lsp_server::LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(graphox::Backend::new(client, config))
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..100 {
        let uri = tower_lsp_server::ls_types::Uri::from_file_path(
            base_dir.join(format!("query_{}.graphql", i)),
        )
        .unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = crate::support::measure_allocated_bytes();
    let delta = used.saturating_sub(baseline);

    println!("Live heap for 100 cached documents: {} KB", delta / 1024);
    assert!(
        delta < MAX_HEAP_CACHED_100_DOCS,
        "Live heap exceeded limit for 100 cached documents: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_HEAP_CACHED_100_DOCS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_schema_caching() {
    let _lock = PERF_MEMORY_MUTEX.lock().await;
    let baseline = crate::support::measure_allocated_bytes();

    let mut temp_dirs = Vec::new();
    let mut temp_dir_holders = Vec::new();
    for i in 0..10 {
        let temp_dir = TempDir::new().unwrap();
        temp_dirs.push(temp_dir.path().to_path_buf());
        temp_dir_holders.push(temp_dir);

        let schema = create_large_schema(200);
        fs::write(temp_dirs[i].join("schema.graphql"), schema).unwrap();

        for j in 0..10 {
            let query = format!("query Query{} {{ item{} {{ id }} }}", j, j % 200);
            fs::write(temp_dirs[i].join(format!("query_{}.graphql", j)), query).unwrap();
        }
    }

    for (i, base_dir) in temp_dirs.iter().enumerate() {
        let config = create_10_schema_config(base_dir);
        let (mut service, _) = tower_lsp_server::LspService::new(|client| {
            graphox::GraphoxLanguageServer::new(graphox::Backend::new(client, config))
        });
        crate::support::lsp_initialize_sequence(&mut service).await;

        for j in 0..10 {
            let uri = tower_lsp_server::ls_types::Uri::from_file_path(
                base_dir.join(format!("query_{}.graphql", j)),
            )
            .unwrap();
            let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", j))).unwrap();
            crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        if i < 9 {
            drop(service);
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = crate::support::measure_allocated_bytes();
    let delta = used.saturating_sub(baseline);

    println!(
        "Live heap for 10 cached schemas (100 docs each): {} KB",
        delta / 1024
    );
    assert!(
        delta < MAX_HEAP_10_SCHEMAS,
        "Live heap exceeded limit for 10 cached schemas: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_HEAP_10_SCHEMAS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_fragment_index() {
    let _lock = PERF_MEMORY_MUTEX.lock().await;
    let baseline = crate::support::measure_allocated_bytes();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    let mut all_fragments = String::new();
    for i in 0..500 {
        let fragment = format!(
            "fragment Frag{} on Query {{ item{} {{ id }} }}\n",
            i,
            i % 100
        );
        all_fragments.push_str(&fragment);
    }
    fs::write(base_dir.join("fragments.graphql"), &all_fragments).unwrap();

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = tower_lsp_server::LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(graphox::Backend::new(client, config))
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    let uri = tower_lsp_server::ls_types::Uri::from_file_path(base_dir.join("fragments.graphql"))
        .unwrap();
    crate::support::lsp_did_open(&mut service, uri.clone(), "graphql", 1, &all_fragments).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = crate::support::measure_allocated_bytes();
    let delta = used.saturating_sub(baseline);

    println!("Live heap for 500 fragment index: {} KB", delta / 1024);
    assert!(
        delta < MAX_HEAP_500_FRAGMENTS,
        "Live heap exceeded limit for 500 fragment index: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_HEAP_500_FRAGMENTS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_large_schema() {
    let _lock = PERF_MEMORY_MUTEX.lock().await;
    let baseline = crate::support::measure_allocated_bytes();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let large_schema = create_large_schema(1000);
    fs::write(base_dir.join("schema.graphql"), large_schema).unwrap();

    for i in 0..50 {
        let query = format!("query Query{} {{ item{} {{ id name }} }}", i, i % 1000);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = tower_lsp_server::LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(graphox::Backend::new(client, config))
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..50 {
        let uri = tower_lsp_server::ls_types::Uri::from_file_path(
            base_dir.join(format!("query_{}.graphql", i)),
        )
        .unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = crate::support::measure_allocated_bytes();
    let delta = used.saturating_sub(baseline);

    println!("Live heap for 1000-type schema: {} KB", delta / 1024);
    assert!(
        delta < MAX_HEAP_LARGE_SCHEMA,
        "Live heap exceeded limit for large schema: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_HEAP_LARGE_SCHEMA / 1024
    );
}
