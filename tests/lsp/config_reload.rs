use crate::support::{create_service, lsp_did_open, lsp_initialize_sequence};
use graphox::Config;
use std::fs;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

/// This test verifies that config file changes are processed without errors.
/// The actual reload happens asynchronously, so we verify that:
/// 1. The system accepts config file change notifications
/// 2. No errors occur during processing
/// 3. The LSP continues to function after config reload
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_config_file_change_triggers_reload() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create initial schema and config
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
"#,
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetUser { user { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config::load_from_dir(&base_dir).unwrap().unwrap();
    let (mut service, _handle) = create_service(config);

    lsp_initialize_sequence(&mut service).await;

    // Modify config file - add output_dir
    fs::write(
        &config_path,
        r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
    output_dir: "generated"
"#,
    )
    .unwrap();

    // Simulate file watcher notification for config file change
    let changes = vec![FileEvent {
        uri: Url::from_file_path(&config_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }];

    let result = service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
                .finish(),
        )
        .await;

    // Should not error - config reload should work
    assert!(result.is_ok(), "Config reload should work without errors");

    // Wait for config reload to complete (triggers workspace scan)
    sleep(Duration::from_millis(10)).await;

    // Verify LSP still works by opening a document
    let doc_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, doc_uri, "graphql", 1, query_text).await;
}

/// This test verifies that invalid config changes are handled gracefully.
/// The LSP should:
/// 1. Detect the config file change
/// 2. Attempt to reload but fail gracefully
/// 3. Continue operating with the old config
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_invalid_config_reload_fails_gracefully() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create initial valid config
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
"#,
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetUser { user { id } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config::load_from_dir(&base_dir).unwrap().unwrap();
    let (mut service, _handle) = create_service(config);

    lsp_initialize_sequence(&mut service).await;

    // Write invalid YAML to config file
    fs::write(&config_path, "this is not valid yaml: [unclosed bracket").unwrap();

    // Simulate file watcher notification
    let changes = vec![FileEvent {
        uri: Url::from_file_path(&config_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }];

    let result = service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
                .finish(),
        )
        .await;

    // Should not error even with invalid config
    assert!(
        result.is_ok(),
        "LSP should handle invalid config gracefully"
    );

    sleep(Duration::from_millis(10)).await;

    // LSP should still work with old config - test by opening a document
    let doc_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, doc_uri, "graphql", 1, query_text).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_config_file_changes_work() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create schema and config
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
"#,
    )
    .unwrap();

    let config = Config::load_from_dir(&base_dir).unwrap().unwrap();
    let (mut service, _handle) = create_service(config);

    lsp_initialize_sequence(&mut service).await;

    // Change a schema file (not config)
    fs::write(
        &schema_path,
        "type Query { post: Post } type Post { id: ID! title: String }",
    )
    .unwrap();

    // Simulate file watcher notification for schema change
    let changes = vec![FileEvent {
        uri: Url::from_file_path(&schema_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }];

    let result = service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
                .finish(),
        )
        .await;

    // Should not error - schema reload should work
    assert!(result.is_ok(), "Schema reload should work without errors");
}
