use graphql_rust::{Backend, Config};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, pos, write_project_file,
};

#[tokio::test]
async fn test_completion_trigger_on_new_empty_line_in_selection() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }",
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
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.graphql");
    let text = "query {\n  users {\n    id\n    \n  }\n}\n";
    fs::write(&query_path, text).unwrap();
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

    let position = Position::new(2, 4);
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
        assert!(items.iter().any(|i| i.label == "id"));
        assert!(items.iter().any(|i| i.label == "username"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_trigger_after_typing_first_character() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }",
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
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.graphql");
    let text = "query {\n  users {\n    id\n    u\n  }\n}\n";
    fs::write(&query_path, text).unwrap();
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

    let position = Position::new(2, 5);
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
        let username_item = items
            .iter()
            .find(|i| i.label == "username")
            .expect("Expected 'username' completion");
        assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(username_item.detail.as_deref(), Some("String!"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_in_completely_empty_selection_set() {
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
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.graphql");
    let text = "query { users { } }";
    fs::write(&query_path, text).unwrap();
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

    let position = Position::new(0, 16);
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
        let id_item = items
            .iter()
            .find(|i| i.label == "id")
            .expect("Expected 'id' completion");
        assert_eq!(id_item.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(id_item.detail.as_deref(), Some("ID!"));

        let username_item = items
            .iter()
            .find(|i| i.label == "username")
            .expect("Expected 'username' completion");
        assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(username_item.detail.as_deref(), Some("String!"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_tsx_trigger_on_new_empty_line_in_selection() {
    let schema =
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    id\n    \n  }\n}\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    let position = Position::new(4, 4);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
async fn test_completion_tsx_trigger_after_typing_first_character() {
    let schema =
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text =
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    id\n    u\n  }\n}\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    let position = Position::new(4, 5);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let username_item = items
        .iter()
        .find(|i| i.label == "username")
        .expect("Expected 'username' completion");
    assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(username_item.detail.as_deref(), Some("String!"));
}

#[tokio::test]
async fn test_completion_tsx_in_completely_empty_selection_set() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "const q = graphql(/* GraphQL */ `\nquery { users { } }\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    let position = Position::new(1, 16);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let username_item = items
        .iter()
        .find(|i| i.label == "username")
        .expect("Expected 'username' completion");
    assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(username_item.detail.as_deref(), Some("String!"));
}

#[tokio::test]
async fn test_completion_operation_type_keywords() {
    let schema = "type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "qu";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(0, 2)).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "query"));
    assert!(items.iter().any(|i| i.label == "mutation"));
    assert!(items.iter().any(|i| i.label == "subscription"));
}

#[tokio::test]
async fn test_completion_schema_keywords() {
    let schema = "type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "ty";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(0, 2)).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "type"));
    assert!(items.iter().any(|i| i.label == "input"));
}

#[tokio::test]
async fn test_completion_union_members() {
    let schema = "type A { id: ID } type B { id: ID } type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "union MyUnion = A | ";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(0, 20)).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "B"));
    assert!(items.iter().any(|i| i.label == "Query"));
}

#[tokio::test]
async fn test_completion_implements_interfaces() {
    let schema = "interface Node { id: ID } type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "type User implements ";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(0, 21)).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "Node"));
}

#[tokio::test]
async fn test_completion_directive_arguments() {
    let schema = "directive @myDir(arg1: String, arg2: Int) on FIELD type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { id @myDir( }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(0, 18)).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "arg1"));
    assert!(items.iter().any(|i| i.label == "arg2"));
}

#[tokio::test]
async fn test_completion_field_alias() {
    let schema = "type User { id: ID name: String } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { user { alias:  } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(0, 22)).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "name"));
}
