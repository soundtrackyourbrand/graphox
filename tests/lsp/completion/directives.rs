use graphql_rust::{Backend, Config};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_completion_directives_on_field() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! } directive @testDirective on FIELD",
    )
    .unwrap();

    let config = Config {
        projects: vec![graphql_rust::config::ProjectConfig {
            schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphql_rust::config::GlobPattern::Single("test.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    let (text, position) = with_cursor("query { users { id @| } }");
    let query_path = dir.path().join("test.graphql");
    fs::write(&query_path, &text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "testDirective"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_directives_on_fragment() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![graphql_rust::config::ProjectConfig {
            schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphql_rust::config::GlobPattern::Single("test.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    let (text, position) = with_cursor("fragment MyFrag on User @| { id }");
    let query_path = dir.path().join("test.graphql");
    fs::write(&query_path, &text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "public"));
        assert!(items.iter().any(|i| i.label == "type_only"));
    } else {
        panic!("Expected array of completions");
    }
}

fn with_cursor(text: &str) -> (String, Position) {
    crate::support::with_cursor(text)
}
