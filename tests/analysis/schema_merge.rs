use crate::support::ProjectConfigBuilder;
use crate::support::builders::ConfigBuilder;
use crate::support::{self, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp::lsp_types::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_merge() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema1.graphql",
            "type User { id: ID! } type Query { me: User }",
        )
        .with_file("schema2.graphql", "type User { name: String }")
        .with_file("query.graphql", "query { me { id name } }");

    let base_dir = scenario.write_files().unwrap();

    let config = ConfigBuilder::new(&base_dir)
        .add_project(
            ProjectConfigBuilder::new()
                .multi_schema(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ])
                .include_pattern("query.graphql")
                .codegen(false),
        )
        .enable_schema_cache(true)
        .build();

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // If merging works, diagnostics should be empty.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty(), "Should have received diagnostics");
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();

    assert!(
        last_diags.is_empty(),
        "Expected no errors with merged schema, but got: {:?}",
        last_diags
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_extension_first() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema1.graphql", "extend type User { name: String }")
        .with_file(
            "schema2.graphql",
            "type User { id: ID! } type Query { me: User }",
        )
        .with_file("query.graphql", "query { me { id name } }");

    let base_dir = scenario.write_files().unwrap();

    let config = ConfigBuilder::new(&base_dir)
        .add_project(
            ProjectConfigBuilder::new()
                .multi_schema(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ])
                .include_pattern("query.graphql")
                .codegen(false),
        )
        .enable_schema_cache(true)
        .build();

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();

    assert!(
        last_diags.is_empty(),
        "Expected no errors when extension comes before base definition, but got: {:?}",
        last_diags
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_with_docs() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema1.graphql",
            "type User { id: ID! } type Query { me: User }",
        )
        .with_file(
            "schema2.graphql",
            "\"\"\"User doc\"\"\"\ntype User { name: String }",
        )
        .with_file("query.graphql", "query { me { id name } }");

    let base_dir = scenario.write_files().unwrap();

    let config = ConfigBuilder::new(&base_dir)
        .add_project(
            ProjectConfigBuilder::new()
                .multi_schema(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ])
                .include_pattern("query.graphql")
                .codegen(false),
        )
        .enable_schema_cache(true)
        .build();

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();

    assert!(
        last_diags.is_empty(),
        "Expected no errors when merging schemas with documentation, but got: {:?}",
        last_diags
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_duplicate_scalars() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema1.graphql",
            "scalar DateTime\ntype Query { now: DateTime }",
        )
        .with_file("schema2.graphql", "scalar DateTime")
        .with_file("query.graphql", "query { now }");

    let base_dir = scenario.write_files().unwrap();

    let config = ConfigBuilder::new(&base_dir)
        .add_project(
            ProjectConfigBuilder::new()
                .multi_schema(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ])
                .include_pattern("query.graphql")
                .codegen(false),
        )
        .enable_schema_cache(true)
        .build();

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { now }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();

    assert!(
        last_diags.is_empty(),
        "Expected no errors when merging schemas with duplicate scalars, but got: {:?}",
        last_diags
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_triple_overlap() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema1.graphql",
            "type User { id: ID! } type Query { me: User }",
        )
        .with_file("schema2.graphql", "type User { name: String }")
        .with_file("schema3.graphql", "type User { age: Int }")
        .with_file("query.graphql", "query { me { id name age } }");

    let base_dir = scenario.write_files().unwrap();

    let config = ConfigBuilder::new(&base_dir)
        .add_project(
            ProjectConfigBuilder::new()
                .multi_schema(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                    "schema3.graphql".to_string(),
                ])
                .include_pattern("query.graphql")
                .codegen(false),
        )
        .enable_schema_cache(true)
        .build();

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name age } }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();

    assert!(
        last_diags.is_empty(),
        "Expected no errors when merging 3 schemas with overlapping User type, but got: {:?}",
        last_diags
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_extension_first_separate_files() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema1.graphql", "extend type User { name: String }")
        .with_file(
            "schema2.graphql",
            "type User { id: ID! } type Query { me: User }",
        )
        .with_file("query.graphql", "query { me { id name } }");

    let base_dir = scenario.write_files().unwrap();

    let config = ConfigBuilder::new(&base_dir)
        .add_project(
            ProjectConfigBuilder::new()
                .multi_schema(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ])
                .include_pattern("query.graphql")
                .codegen(false),
        )
        .enable_schema_cache(true)
        .build();

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();

    assert!(
        last_diags.is_empty(),
        "Expected no errors when extension is in schema1 and base is in schema2, but got: {:?}",
        last_diags
    );
}
