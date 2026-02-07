use crate::support::{self, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use graphql_rust::{
    config::GlobPattern, config::ProjectConfig, config::SchemaSource, Config,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::lsp_types::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_merge() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(
        &schema1_path,
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "type User { name: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec![
                "schema1.graphql".to_string(),
                "schema2.graphql".to_string(),
            ]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(&schema1_path, "extend type User { name: String }").unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(
        &schema2_path,
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec![
                "schema1.graphql".to_string(),
                "schema2.graphql".to_string(),
            ]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(
        &schema1_path,
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(
        &schema2_path,
        "\"\"\"User doc\"\"\"\ntype User { name: String }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec![
                "schema1.graphql".to_string(),
                "schema2.graphql".to_string(),
            ]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(
        &schema1_path,
        "scalar DateTime\ntype Query { now: DateTime }",
    )
    .unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "scalar DateTime").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { now }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec![
                "schema1.graphql".to_string(),
                "schema2.graphql".to_string(),
            ]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(
        &schema1_path,
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "type User { name: String }").unwrap();

    let schema3_path = base_dir.join("schema3.graphql");
    fs::write(&schema3_path, "type User { age: Int }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name age } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec![
                "schema1.graphql".to_string(),
                "schema2.graphql".to_string(),
                "schema3.graphql".to_string(),
            ]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(&schema1_path, "extend type User { name: String }").unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(
        &schema2_path,
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec![
                "schema1.graphql".to_string(),
                "schema2.graphql".to_string(),
            ]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

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