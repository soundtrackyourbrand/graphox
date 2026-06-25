//! Memory stress tests.
//! These tests verify memory usage under various stress conditions.

use crate::support::{create_large_schema, measure_memory_usage};
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const MAX_MEMORY_100_DOCS: usize = 60 * 1024 * 1024; // 60MB
const MAX_MEMORY_10_SCHEMAS: usize = 10 * 1024 * 1024; // 10MB
const MAX_MEMORY_500_FRAGMENTS: usize = 40 * 1024 * 1024; // 40MB

const BASELINE_MEMORY_100_DOCS: usize = 50 * 1024 * 1024;
const BASELINE_MEMORY_10_SCHEMAS: usize = 10 * 1024 * 1024;
const BASELINE_MEMORY_500_FRAGMENTS: usize = 35 * 1024 * 1024;

const PER_DOC_BUDGET: usize = MAX_MEMORY_100_DOCS / 100;
const PER_SCHEMA_BUDGET: usize = MAX_MEMORY_10_SCHEMAS / 10;
const PER_FRAGMENT_BUDGET: usize = MAX_MEMORY_500_FRAGMENTS / 500;

const ALLOWED_GROWTH_PERCENT: f64 = 5.0;

// Live heap for this workspace measures ~69 MB; 90 MB leaves headroom for
// allocation-size jitter and platform variation while still catching a gross
// regression in per-document/per-schema retention.
const MAX_MEMORY_COMPLEX_MONOREPO: usize = 90 * 1024 * 1024;

use once_cell::sync::Lazy;
use tokio::sync::Mutex;

static MEMORY_TEST_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// Warm up the allocator by doing some allocations and frees.
/// This helps stabilize the RSS baseline.
fn warmup() {
    for _ in 0..5 {
        let mut v = Vec::with_capacity(5000000);
        for i in 0..5000000 {
            v.push(i);
        }
        drop(v);
    }
}

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
    let _lock = MEMORY_TEST_MUTEX.lock().await;
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
    let (mut service, _) =
        tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
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
    let _lock = MEMORY_TEST_MUTEX.lock().await;
    warmup();
    let baseline = measure_memory_usage();

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
        let (mut service, _) =
            tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
        crate::support::lsp_initialize_sequence(&mut service).await;

        for i in 0..20 {
            let uri = tower_lsp::lsp_types::Url::from_file_path(
                base_dir.join(format!("query_{}.graphql", i)),
            )
            .unwrap();
            let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
            crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!(
        "Memory after 10 open/close cycles (100 files each): {} KB",
        delta / 1024
    );
    assert!(
        delta < 40 * 1024 * 1024,
        "Memory exceeded limit for 10 open/close cycles (100 files each): {} KB",
        delta / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_memory_cached_documents_100() {
    let _lock = MEMORY_TEST_MUTEX.lock().await;
    warmup();
    let baseline = measure_memory_usage();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..100 {
        let query = format!("query Query{} {{ item{} {{ id name }} }}", i, i % 100);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) =
        tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..100 {
        let uri = tower_lsp::lsp_types::Url::from_file_path(
            base_dir.join(format!("query_{}.graphql", i)),
        )
        .unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 100 cached documents: {} KB", delta / 1024);
    assert!(
        delta <= PER_DOC_BUDGET * 100,
        "Memory exceeded per-doc budget: {} KB (budget: {} KB)",
        delta / 1024,
        (PER_DOC_BUDGET * 100) / 1024
    );

    let growth =
        (delta as f64 - BASELINE_MEMORY_100_DOCS as f64) / BASELINE_MEMORY_100_DOCS as f64 * 100.0;
    if delta > BASELINE_MEMORY_100_DOCS {
        assert!(
            growth <= ALLOWED_GROWTH_PERCENT,
            "Memory growth exceeded limit: {:.2}% (limit: {:.2}%)",
            growth,
            ALLOWED_GROWTH_PERCENT
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_schema_caching() {
    let _lock = MEMORY_TEST_MUTEX.lock().await;
    warmup();
    let baseline = measure_memory_usage();

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
        let (mut service, _) =
            tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
        crate::support::lsp_initialize_sequence(&mut service).await;

        for j in 0..10 {
            let uri = tower_lsp::lsp_types::Url::from_file_path(
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

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!(
        "Memory for 10 cached schemas (100 docs each): {} KB",
        delta / 1024
    );
    assert!(
        delta <= PER_SCHEMA_BUDGET * 10,
        "Memory exceeded per-schema budget: {} KB (budget: {} KB)",
        delta / 1024,
        (PER_SCHEMA_BUDGET * 10) / 1024
    );

    let growth = (delta as f64 - BASELINE_MEMORY_10_SCHEMAS as f64)
        / BASELINE_MEMORY_10_SCHEMAS as f64
        * 100.0;
    if delta > BASELINE_MEMORY_10_SCHEMAS {
        assert!(
            growth <= ALLOWED_GROWTH_PERCENT,
            "Memory growth exceeded limit for 10 schemas: {:.2}% (limit: {:.2}%)",
            growth,
            ALLOWED_GROWTH_PERCENT
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_fragment_index() {
    let _lock = MEMORY_TEST_MUTEX.lock().await;
    warmup();
    let baseline = measure_memory_usage();

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
    let (mut service, _) =
        tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
    crate::support::lsp_initialize_sequence(&mut service).await;

    let uri =
        tower_lsp::lsp_types::Url::from_file_path(base_dir.join("fragments.graphql")).unwrap();
    crate::support::lsp_did_open(&mut service, uri.clone(), "graphql", 1, &all_fragments).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 500 fragment index: {} KB", delta / 1024);
    assert!(
        delta <= PER_FRAGMENT_BUDGET * 500,
        "Memory exceeded per-fragment budget: {} KB (budget: {} KB)",
        delta / 1024,
        (PER_FRAGMENT_BUDGET * 500) / 1024
    );

    let growth = (delta as f64 - BASELINE_MEMORY_500_FRAGMENTS as f64)
        / BASELINE_MEMORY_500_FRAGMENTS as f64
        * 100.0;
    if delta > BASELINE_MEMORY_500_FRAGMENTS {
        assert!(
            growth <= ALLOWED_GROWTH_PERCENT,
            "Memory growth exceeded limit for 500 fragments: {:.2}% (limit: {:.2}%)",
            growth,
            ALLOWED_GROWTH_PERCENT
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_large_schema() {
    let _lock = MEMORY_TEST_MUTEX.lock().await;
    warmup();
    let baseline = measure_memory_usage();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let large_schema = create_large_schema(1000);
    fs::write(base_dir.join("schema.graphql"), large_schema).unwrap();

    for i in 0..50 {
        let query = format!("query Query{} {{ item{} {{ id name }} }}", i, i % 1000);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) =
        tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..50 {
        let uri = tower_lsp::lsp_types::Url::from_file_path(
            base_dir.join(format!("query_{}.graphql", i)),
        )
        .unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 1000-type schema: {} KB", delta / 1024);
    assert!(
        delta < 40 * 1024 * 1024,
        "Memory exceeded limit for large schema: {} KB",
        delta / 1024
    );
}
