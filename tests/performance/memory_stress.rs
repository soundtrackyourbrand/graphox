//! Memory stress tests.
//! These tests verify memory usage under various stress conditions.

use crate::support::lsp::LspTestScenario;
use crate::support::{create_large_schema, measure_memory_usage};
use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphql_rust::Config;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

const MAX_MEMORY_100_DOCS: usize = 50 * 1024 * 1024; // 50MB
const MAX_MEMORY_1000_DOCS: usize = 200 * 1024 * 1024; // 200MB
const MAX_MEMORY_50_SCHEMAS: usize = 100 * 1024 * 1024; // 100MB
const MAX_MEMORY_500_FRAGMENTS: usize = 50 * 1024 * 1024; // 50MB

fn create_100_file_config(base_dir: &PathBuf) -> Config {
    Config {
        base_dir: base_dir.clone(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    }
}

fn create_1000_file_config(base_dir: &PathBuf) -> Config {
    Config {
        base_dir: base_dir.clone(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    }
}

fn create_10_schema_config(base_dir: &PathBuf) -> Config {
    Config {
        base_dir: base_dir.clone(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_open_close_cycles() {
    let baseline = measure_memory_usage();

    for _cycle in 0..5 {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let schema = create_large_schema(100);
        fs::write(base_dir.join("schema.graphql"), schema).unwrap();

        for i in 0..20 {
            let query = format!(
                "query Query{} {{ item{} {{ id }} }}",
                i,
                i % 100
            );
            fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
        }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = tower_lsp::LspService::new(|client| {
        graphql_rust::Backend::new(client, config)
    });
        crate::support::lsp_initialize_sequence(&mut service).await;

        for i in 0..20 {
            let uri = tower_lsp::lsp_types::Url::from_file_path(
                base_dir.join(format!("query_{}.graphql", i))
            ).unwrap();
            let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
            crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory after 5 open/close cycles (100 files each): {} KB", delta / 1024);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(120)]
async fn test_memory_cached_documents_100() {
    let baseline = measure_memory_usage();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..100 {
        let query = format!(
            "query Query{} {{ item{} {{ id name }} }}",
            i,
            i % 100
        );
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = graphql_rust::LspService::new(|client| {
        graphql_rust::Backend::new(client, config)
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..100 {
        let uri = tower_lsp::lsp_types::Url::from_file_path(
            base_dir.join(format!("query_{}.graphql", i))
        ).unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 100 cached documents: {} KB", delta / 1024);
    assert!(
        delta < MAX_MEMORY_100_DOCS,
        "Memory exceeded limit for 100 documents: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_MEMORY_100_DOCS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(300)]
async fn test_memory_cached_documents_1000() {
    let baseline = measure_memory_usage();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..1000 {
        let query = format!(
            "query Query{} {{ item{} {{ id name email }} }}",
            i,
            i % 100
        );
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_1000_file_config(&base_dir);
    let (mut service, _) = tower_lsp::LspService::new(|client| {
        graphql_rust::Backend::new(client, config)
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..1000 {
        let uri = tower_lsp::lsp_types::Url::from_file_path(
            base_dir.join(format!("query_{}.graphql", i))
        ).unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 1000 cached documents: {} KB", delta / 1024);
    assert!(
        delta < MAX_MEMORY_1000_DOCS,
        "Memory exceeded limit for 1000 documents: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_MEMORY_1000_DOCS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_schema_caching() {
    let baseline = measure_memory_usage();

    let mut temp_dirs = Vec::new();
    for i in 0..10 {
        let temp_dir = TempDir::new().unwrap();
        temp_dirs.push(temp_dir.path().to_path_buf());

        let schema = create_large_schema(200);
        fs::write(temp_dirs[i].join("schema.graphql"), schema).unwrap();

        for j in 0..10 {
            let query = format!(
                "query Query{} {{ item{} {{ id }} }}",
                j,
                j % 200
            );
            fs::write(temp_dirs[i].join(format!("query_{}.graphql", j)), query).unwrap();
        }
    }

    for (i, base_dir) in temp_dirs.iter().enumerate() {
    let config = create_10_schema_config(base_dir);
    let (mut service, _) = tower_lsp::LspService::new(|client| {
            graphql_rust::Backend::new(client, config)
        });
        crate::support::lsp_initialize_sequence(&mut service).await;

        for j in 0..10 {
            let uri = tower_lsp::lsp_types::Url::from_file_path(
                base_dir.join(format!("query_{}.graphql", j))
            ).unwrap();
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

    println!("Memory for 10 cached schemas (100 docs each): {} KB", delta / 1024);
    assert!(
        delta < MAX_MEMORY_50_SCHEMAS,
        "Memory exceeded limit for 10 schemas: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_MEMORY_50_SCHEMAS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_fragment_index() {
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
    fs::write(base_dir.join("fragments.graphql"), all_fragments).unwrap();

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = graphql_rust::LspService::new(|client| {
        graphql_rust::Backend::new(client, config)
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    let uri = graphql_rsp::lsp_types::Url::from_file_path(
        base_dir.join("fragments.graphql")
    ).unwrap();
    crate::support::lsp_did_open(&mut service, uri.clone(), "graphql", 1, &all_fragments).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 500 fragment index: {} KB", delta / 1024);
    assert!(
        delta < MAX_MEMORY_500_FRAGMENTS,
        "Memory exceeded limit for 500 fragments: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_MEMORY_500_FRAGMENTS / 1024
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_memory_large_schema() {
    let baseline = measure_memory_usage();

    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let large_schema = create_large_schema(1000);
    fs::write(base_dir.join("schema.graphql"), large_schema).unwrap();

    for i in 0..50 {
        let query = format!(
            "query Query{} {{ item{} {{ id name }} }}",
            i,
            i % 1000
        );
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let (mut service, _) = graphql_rust::LspService::new(|client| {
        graphql_rust::Backend::new(client, config)
    });
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..50 {
        let uri = tower_lsp::lsp_types::Url::from_file_path(
            base_dir.join(format!("query_{}.graphql", i))
        ).unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let used = measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 1000-type schema: {} KB", delta / 1024);
    assert!(
        delta < 150 * 1024 * 1024,
        "Memory exceeded limit for large schema: {} KB",
        delta / 1024
    );
}
