use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use graphql_rust::{Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_merge() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(&schema1_path, "type User { id: ID! } type Query { me: User }").unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "type User { name: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec!["schema1.graphql".to_string(), "schema2.graphql".to_string()]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        base_dir: base_dir.clone(),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap();

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    // If merging works, diagnostics should be empty.
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty(), "Should have received diagnostics");
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();
    
    assert!(last_diags.is_empty(), "Expected no errors with merged schema, but got: {:?}", last_diags);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_extension_first() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(&schema1_path, "extend type User { name: String }").unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "type User { id: ID! } type Query { me: User }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec!["schema1.graphql".to_string(), "schema2.graphql".to_string()]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        base_dir: base_dir.clone(),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap();

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();
    
    assert!(last_diags.is_empty(), "Expected no errors when extension comes before base definition, but got: {:?}", last_diags);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_with_docs() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(&schema1_path, "type User { id: ID! } type Query { me: User }").unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "\"\"\"User doc\"\"\"\ntype User { name: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec!["schema1.graphql".to_string(), "schema2.graphql".to_string()]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        base_dir: base_dir.clone(),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap();

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();
    
    assert!(last_diags.is_empty(), "Expected no errors when merging schemas with documentation, but got: {:?}", last_diags);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_multi_schema_duplicate_scalars() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema1_path = base_dir.join("schema1.graphql");
    fs::write(&schema1_path, "scalar DateTime\ntype Query { now: DateTime }").unwrap();

    let schema2_path = base_dir.join("schema2.graphql");
    fs::write(&schema2_path, "scalar DateTime").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { now }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Multiple(vec!["schema1.graphql".to_string(), "schema2.graphql".to_string()]),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        base_dir: base_dir.clone(),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap();

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    let diags = received_diags.lock().unwrap();
    assert!(!diags.is_empty());
    let last_diags = diags.last().unwrap()["diagnostics"].as_array().unwrap();
    
    assert!(last_diags.is_empty(), "Expected no errors when merging schemas with duplicate scalars, but got: {:?}", last_diags);
}
