//! Large workspace performance tests.
//! These tests verify performance with many files and complex workspace structures.

use crate::support::{
    create_deep_fragment_chain, create_large_schema, create_many_fragments, lsp_initialize_sequence,
};
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};
use std::fs;
use tempfile::TempDir;
use tower_lsp::LspService;
use tower_lsp::lsp_types::Url;

const MAX_SCAN_TIME: std::time::Duration = std::time::Duration::from_secs(2);

fn create_100_file_config(base_dir: &std::path::Path) -> Config {
    Config::new_test(
        base_dir.to_path_buf(),
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

fn create_500_file_config(base_dir: &std::path::Path) -> Config {
    Config::new_test(
        base_dir.to_path_buf(),
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

fn create_1000_file_config(base_dir: &std::path::Path) -> Config {
    Config::new_test(
        base_dir.to_path_buf(),
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

fn create_10_project_config(base_dir: &std::path::Path) -> Config {
    let mut projects = Vec::new();
    for i in 0..10 {
        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Single(format!(
                    "project_{}/schema.graphql",
                    i
                )))
                .with_include(GlobPattern::Single(format!("project_{}/**/*.graphql", i)))
                .with_codegen(CodegenConfig::disabled()),
        );
    }
    Config::new_test(base_dir.to_path_buf(), projects)
        .with_enable_schema_cache(true)
        .with_lsp_automatic_codegen(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_workspace_scan_100_files() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..100 {
        let query = format!("query Query{} {{ item{} {{ id }} }}", i, i % 100);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_100_file_config(&base_dir);
    let start = std::time::Instant::now();
    let (mut service, _) = LspService::new(|client| graphox::Backend::new(client, config.clone()));
    lsp_initialize_sequence(&mut service).await;
    let duration = start.elapsed();

    println!("Workspace scan (100 files) took: {:?}", duration);
    assert!(
        duration < MAX_SCAN_TIME,
        "100-file workspace scan took too long: {:?}",
        duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_workspace_scan_500_files() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..500 {
        let query = format!("query Query{} {{ item{} {{ id name }} }}", i, i % 100);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_500_file_config(&base_dir);
    let start = std::time::Instant::now();
    let (mut service, _) = LspService::new(|client| graphox::Backend::new(client, config.clone()));
    lsp_initialize_sequence(&mut service).await;
    let duration = start.elapsed();

    println!("Workspace scan (500 files) took: {:?}", duration);
    assert!(
        duration < std::time::Duration::from_secs(15),
        "500-file workspace scan took too long: {:?}",
        duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_workspace_scan_1000_files() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..1000 {
        let query = format!("query Query{} {{ item{} {{ id }} }}", i, i % 100);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_1000_file_config(&base_dir);
    let start = std::time::Instant::now();
    let (mut service, _) = LspService::new(|client| graphox::Backend::new(client, config.clone()));
    lsp_initialize_sequence(&mut service).await;
    let duration = start.elapsed();

    println!("Workspace scan (1000 files) took: {:?}", duration);
    assert!(
        duration < std::time::Duration::from_secs(30),
        "1000-file workspace scan took too long: {:?}",
        duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(2000)]
async fn test_fragment_resolution_chain() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    let deep_chain = create_deep_fragment_chain(50);
    fs::write(base_dir.join("deep_chain.graphql"), deep_chain).unwrap();

    let config = create_100_file_config(&base_dir);
    let start = std::time::Instant::now();
    let (mut service, _) = LspService::new(|client| graphox::Backend::new(client, config.clone()));
    lsp_initialize_sequence(&mut service).await;

    let uri = Url::from_file_path(base_dir.join("deep_chain.graphql")).unwrap();
    crate::support::lsp_request_hover(&mut service, uri, crate::support::pos(0, 0)).await;
    let duration = start.elapsed();

    println!(
        "Deep fragment chain resolution (50 levels) took: {:?}",
        duration
    );
    assert!(
        duration < std::time::Duration::from_secs(1),
        "Fragment chain resolution took too long: {:?}",
        duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(2000)]
async fn test_many_projects() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    for i in 0..10 {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        let schema = create_large_schema(50);
        fs::write(project_dir.join("schema.graphql"), schema).unwrap();

        for j in 0..20 {
            let query = format!("query Query{} {{ item{} {{ id }} }}", j, j % 50);
            fs::write(project_dir.join(format!("query_{}.graphql", j)), query).unwrap();
        }
    }

    let config = create_10_project_config(&base_dir);
    let start = std::time::Instant::now();
    let (mut service, _) = LspService::new(|client| graphox::Backend::new(client, config.clone()));
    lsp_initialize_sequence(&mut service).await;
    let duration = start.elapsed();

    println!("10-project workspace scan took: {:?}", duration);
    assert!(
        duration < std::time::Duration::from_secs(1),
        "10-project workspace scan took too long: {:?}",
        duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(2000)]
async fn test_many_fragments_index() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    let fragments = create_many_fragments(500);
    fs::write(base_dir.join("fragments.graphql"), fragments).unwrap();

    let config = create_100_file_config(&base_dir);
    let start = std::time::Instant::now();
    let (mut service, _) = LspService::new(|client| graphox::Backend::new(client, config.clone()));
    lsp_initialize_sequence(&mut service).await;
    let duration = start.elapsed();

    println!("500 fragment index build took: {:?}", duration);
    assert!(
        duration < std::time::Duration::from_secs(1),
        "500 fragment index build took too long: {:?}",
        duration
    );
}
