use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

use crate::support::{
    make_temp_project_with_schema, create_initialized_lsp_service, write_project_file,
    lsp_did_open, lsp_request_completion, completion_items_array,
};

#[tokio::test]
async fn test_completion_fields() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    // Create initialized service
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Write and open file
    let text = "query { users {  } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Request completions at "users { | }"
    let position = Position::new(0, 16);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Verify labels and metadata (kind/detail)
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
}

#[tokio::test]
async fn test_inline_fragment_completion_inserts_braces_when_missing() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query {\n  users {\n    ... on \n  }\n}\n";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Cursor after '... on '
    let position = Position::new(2, 11);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items.iter().find(|i| i.label == "User").expect("Expected 'User' completion");

    // Apply completion using helper
    let (final_text, _pos) = crate::support::apply_completion_item(text, position, item);

    assert_eq!(
        final_text,
        "query {\n  users {\n    ... on User {\n      \n    }\n  }\n}\n"
    );
}

#[tokio::test]
async fn test_inline_fragment_completion_no_braces_when_present() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query {\n  users {\n    ... on  { id }\n  }\n}\n";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Cursor after '... on '
    let position = Position::new(2, 11);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    // Apply completion using helper
    let (final_text, _pos) = crate::support::apply_completion_item(text, position, item);

    assert_eq!(
        final_text,
        "query {\n  users {\n    ... on User { id }\n  }\n}\n"
    );
}

#[tokio::test]
async fn test_inline_fragment_completion_tsx_inserts_braces_when_missing() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    ... on \n  }\n}\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    // Cursor in file line corresponding to the inner GraphQL line with inline fragment (file line 3)
    let position = Position::new(3, 11);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    let (final_text, new_pos) = crate::support::apply_completion_item(text, position, item);

    // Expect the snippet expanded (with $0 removed)
    assert!(final_text.contains("... on User {"));

    // Assert final cursor position when snippet provided
    if let Some(pos) = new_pos {
        assert_eq!(pos, Position::new(4, 6));
    } else if let Some(insert_text) = &item.insert_text
        && insert_text.contains("$0")
    {
        panic!("Expected new_pos to be Some when snippet is applied");
    }
}

#[tokio::test]
async fn test_inline_fragment_completion_tsx_no_braces_when_present() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text =
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    ... on  { id }\n  }\n}\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    let position = Position::new(3, 11);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    if item.text_edit.is_none() {
        let insert_text = item.insert_text.as_ref().unwrap_or(&item.label);
        assert!(!insert_text.contains('{'));
    }
}

#[tokio::test]
async fn test_completion_variables() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query GetUser($userId: ID!) { user(id: $) }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Request completions at "user(id: $|)"
    let position = Position::new(0, 40);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "$userId"),
        "Expected $userId in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_fragment_spread() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment MyFrag on User { id } query { users { ... } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Request completions after "..."
    let position = Position::new(0, 50);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "MyFrag"));
}

#[tokio::test]
async fn test_completion_types_in_fragment() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment MyFrag on  { id }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Request completions at "on |"
    let position = Position::new(0, 19);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "User"),
        "Expected User in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_fragment_spread_acceptance() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment MyFrag on User { id }\nquery { users { ... } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Request completions after "..." (which is at line 1, column 19)
    let position = Position::new(1, 19);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "MyFrag")
        .expect("MyFrag completion not found");

    // Apply completion using helper
    let (final_text, _pos) = crate::support::apply_completion_item(text, position, item);

    assert_eq!(
        final_text,
        "fragment MyFrag on User { id }\nquery { users { ...MyFrag } }"
    );
}

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
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "query { users { id @ } }";
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

    // Request completions at "@|"
    let position = Position::new(0, 20);
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
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment MyFrag on User @ { id }";
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

    // Request completions at "@|"
    let position = Position::new(0, 25);
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

#[tokio::test]
async fn test_completion_introspection_fields() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "query {  }";
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

    // Request completions at "query { | }" - should include __schema and __type
    let position = Position::new(0, 8);
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
        // Should have regular fields
        assert!(
            items.iter().any(|i| i.label == "users"),
            "Should include regular field 'users'"
        );

        // Should have __typename (available on all types)
        assert!(
            items.iter().any(|i| i.label == "__typename"),
            "Should include __typename"
        );

        // Should have __schema (Query root only)
        let schema_item = items.iter().find(|i| i.label == "__schema");
        assert!(
            schema_item.is_some(),
            "Should include __schema on Query root"
        );
        if let Some(item) = schema_item {
            assert_eq!(
                item.detail.as_deref(),
                Some("__Schema!"),
                "Should have correct type for __schema"
            );
        }

        // Should have __type (Query root only)
        let type_item = items.iter().find(|i| i.label == "__type");
        assert!(type_item.is_some(), "Should include __type on Query root");
        if let Some(item) = type_item {
            assert_eq!(
                item.detail.as_deref(),
                Some("__Type"),
                "Should have correct type for __type"
            );
        }
    } else {
        panic!("Expected array of completions");
    }

    // Now test that __schema and __type are NOT included on non-root types
    let text2 = "query { users {  } }";
    fs::write(&query_path, text2).unwrap();

    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text2.to_string(),
        }],
    };
    service
        .call(
            Request::build("textDocument/didChange")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // Request completions at "users { | }"
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
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        // Should have regular User fields
        assert!(
            items.iter().any(|i| i.label == "id"),
            "Should include regular field 'id'"
        );
        assert!(
            items.iter().any(|i| i.label == "username"),
            "Should include regular field 'username'"
        );

        // Should still have __typename
        assert!(
            items.iter().any(|i| i.label == "__typename"),
            "Should include __typename on User type"
        );

        // Should NOT have __schema (not Query root)
        assert!(
            !items.iter().any(|i| i.label == "__schema"),
            "Should NOT include __schema on non-root User type"
        );

        // Should NOT have __type (not Query root)
        assert!(
            !items.iter().any(|i| i.label == "__type"),
            "Should NOT include __type on non-root User type"
        );
    } else {
        panic!("Expected array of completions");
    }
}

// New tests for completion triggering in various empty-line/empty-selection scenarios
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
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    // Request completions at the empty new line inside selection (line 3, column 4)
    let position = Position::new(2, 4); // 0-based: line 2 contains the empty line after id
    // Note: depending on exact whitespace counting, column 4 is after indentation
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
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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
    // Put a single character 'u' on the new line to simulate typing
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

    // Request completions after the 'u' we typed (line 3, column 5)
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
        // Should include username (filtered by prefix 'u')
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
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    // Request completions at the empty selection position
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

// Embedded GraphQL in TSX/TypeScript - similar scenarios as GraphQL-only tests
#[tokio::test]
async fn test_completion_tsx_trigger_on_new_empty_line_in_selection() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    id\n    \n  }\n}\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    // Request completions at the empty new line inside selection. The empty line is at file line 4 (0-based), col 4
    let position = Position::new(4, 4);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
async fn test_completion_tsx_trigger_after_typing_first_character() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Put a single character 'u' on the new line to simulate typing
    let text =
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    id\n    u\n  }\n}\n`);\n";
    let uri = write_project_file(&dir, "test.tsx", text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, text).await;

    // Request completions after the 'u' we typed (line 4, column 5)
    let position = Position::new(4, 5);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should include username (filtered by prefix 'u')
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

    // Request completions at the empty selection position. The users selection braces are on line 1 -> position line 1, col 16
    let position = Position::new(1, 16);
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

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
}

#[tokio::test]
async fn test_completion_selection_set_type_filtering() {
    let schema = "type Query { users: [User!]! posts: [Post!]! } type User { id: ID! username: String! } type Post { id: ID! title: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { users {  } posts {  } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // completions in users selection (label should include 'username' but not 'title')
    let users_pos = Position::new(0, 15); // around users { | }
    let result = lsp_request_completion(&mut service, uri.clone(), users_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "username"));
    assert!(!items.iter().any(|i| i.label == "title"));

    // completions in posts selection (label should include 'title' but not 'username')
    let posts_pos = Position::new(0, 26); // around posts { | }
    let result = lsp_request_completion(&mut service, uri.clone(), posts_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "title"));
    assert!(!items.iter().any(|i| i.label == "username"));
}

// Interface fragment spread filtering
#[tokio::test]
async fn test_fragment_spread_interface_filtering() {
    let schema = "type Query { nodeA: A nodeB: B } interface Node { id: ID! } type A implements Node { id: ID! name: String! } type B implements Node { id: ID! title: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment OnNode on Node { id } fragment OnA on A { name } fragment OnB on B { title } query { nodeA { ... } nodeB { ... } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let dot_idx = text.find("nodeA { ...").unwrap();
    let prefix = &text[..dot_idx + "nodeA { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let users_pos = Position::new(line as u32, col as u32);

    let result = lsp_request_completion(&mut service, uri.clone(), users_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "OnA"));
    assert!(items.iter().any(|i| i.label == "OnNode"));
    assert!(!items.iter().any(|i| i.label == "OnB"));
    // ensure no field completions are present for '...'
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FIELD))
    );

    // nodeB position
    let dot_idx = text.find("nodeB { ...").unwrap();
    let prefix = &text[..dot_idx + "nodeB { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let posts_pos = Position::new(line as u32, col as u32);

    let result = lsp_request_completion(&mut service, uri.clone(), posts_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "OnB"));
    assert!(items.iter().any(|i| i.label == "OnNode"));
    assert!(!items.iter().any(|i| i.label == "OnA"));
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FIELD))
    );
}

// Union fragment spread filtering
#[tokio::test]
async fn test_fragment_spread_union_filtering_extended() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { itemA: A itemB: B } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
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
    let text = "fragment OnItem on Item { id } fragment OnA on A { name } fragment OnB on B { title } query { itemA { ... } itemB { ... } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let file_text = text;
    // itemA
    let dot_idx = file_text.find("itemA { ...").unwrap();
    let prefix = &file_text[..dot_idx + "itemA { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos_a = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos_a,
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
        assert!(items.iter().any(|i| i.label == "OnA"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnB"));
    } else {
        panic!("Expected array of completions");
    }

    // itemB
    let dot_idx = file_text.find("itemB { ...").unwrap();
    let prefix = &file_text[..dot_idx + "itemB { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos_b = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos_b,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let request = Request::build("textDocument/completion")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnB"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnA"));
    } else {
        panic!("Expected array of completions");
    }
}

// Embedded TSX: interface fragment spreads
#[tokio::test]
async fn test_embedded_fragment_spreads_interface_tsx() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { node: A item: B } interface Node { id: ID! } type A implements Node { id: ID! name: String! } type B implements Node { id: ID! title: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.tsx".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
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

    let query_path = dir.path().join("test.tsx");
    let text = r#"const q = graphql(/* GraphQL */ `fragment OnNode on Node { id } fragment OnA on A { name } fragment OnB on B { title } query { node { ... } item { ... } }`);"#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let file_text = text.to_string();
    let dot_idx = file_text.find("node { ...").unwrap();
    let prefix = &file_text[..dot_idx + "node { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos,
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
        assert!(items.iter().any(|i| i.label == "OnA"));
        assert!(items.iter().any(|i| i.label == "OnNode"));
        assert!(!items.iter().any(|i| i.label == "OnB"));
    } else {
        panic!("Expected array of completions");
    }
}

// Embedded TSX: union fragment spreads
#[tokio::test]
async fn test_embedded_fragment_spreads_union_tsx() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { item: A other: B } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.tsx".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
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

    let query_path = dir.path().join("test.tsx");
    let text = r#"const q = graphql(/* GraphQL */ `fragment OnItem on Item { id } fragment OnA on A { name } fragment OnB on B { title } query { item { ... } other { ... } }`);"#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let file_text = text.to_string();
    let dot_idx = file_text.find("item { ...").unwrap();
    let prefix = &file_text[..dot_idx + "item { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos,
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
        assert!(items.iter().any(|i| i.label == "OnA"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnB"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_fragment_spread_type_filtering() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! posts: [Post!]! } type User { id: ID! username: String! } type Post { id: ID! title: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    // create a file with two fragments and a query using ... in users
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
    let text = "fragment OnUser on User { id } fragment OnPost on Post { id } query { users { ... } posts { ... } }";
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

    // completions after ... in users should include OnUser but not OnPost or fields
    // compute approximate position by finding index of the first '...' occurrence
    let file_text = text;
    let dot_idx = file_text.find("users { ...").unwrap();
    // count lines and columns up to dot_idx
    let prefix = &file_text[..dot_idx + "users { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let users_pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: users_pos,
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
        assert!(items.iter().any(|i| i.label == "OnUser"));
        assert!(!items.iter().any(|i| i.label == "OnPost"));
        assert!(
            !items
                .iter()
                .any(|i| i.kind == Some(CompletionItemKind::FIELD))
        );
    } else {
        panic!("Expected array of completions");
    }

    // completions after ... in posts should include OnPost but not OnUser
    let dot_idx = file_text.find("posts { ...").unwrap();
    let prefix = &file_text[..dot_idx + "posts { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let posts_pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: posts_pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let request = Request::build("textDocument/completion")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnPost"));
        assert!(!items.iter().any(|i| i.label == "OnUser"));
        assert!(
            !items
                .iter()
                .any(|i| i.kind == Some(CompletionItemKind::FIELD))
        );
    } else {
        panic!("Expected array of completions");
    }
}
