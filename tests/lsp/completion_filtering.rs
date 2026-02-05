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

#[tokio::test]
async fn test_completion_fragment_spread_filtering() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User posts: [Post] } type User { id: ID! } type Post { id: ID! title: String }",
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
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.graphql");
    let text = r#"
        fragment UserFields on User { id }
        fragment PostFields on Post { id title }
        
        query {
            user {
                ...
            }
        }
    "#;
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

    // Request completions at "user { ...| }"
    // Line 6, char 19 (approx)
    let position = Position::new(6, 19);
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

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array"),
    };

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();

    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields on User type. Found: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "PostFields"),
        "Should NOT suggest PostFields on User type. Found: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_fragment_spread_interface_filtering() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "interface Node { id: ID! } type User implements Node { id: ID! name: String } type Query { user: User }",
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
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.graphql");
    let text = r#"
        fragment NodeFields on Node { id }
        fragment UserFields on User { name }
        
        query {
            user {
                ...
            }
        }
    "#;
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

    let position = Position::new(6, 19);
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

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array"),
    };

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();

    assert!(
        items.iter().any(|i| i.label == "NodeFields"),
        "Should suggest NodeFields on User type (interface). Found: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields on User type. Found: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_fragment_spread_union_filtering() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! } type Guest { name: String } union Actor = User | Guest type Query { actor: Actor }",
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
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.graphql");
    let text = r#"
        fragment UserFields on User { id }
        fragment ActorFields on Actor { __typename }
        
        query {
            actor {
                ...
            }
        }
    "#;
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

    let position = Position::new(6, 19);
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

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array"),
    };

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();

    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields on Actor union (member). Found: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "ActorFields"),
        "Should suggest ActorFields on Actor union. Found: {:?}",
        labels
    );

    // Test reverse: spread union fragment into object member
    let updated_text = r#"
        fragment UserFields on User { id }
        fragment GuestFields on Guest { name }
        fragment ActorFields on Actor { __typename }
        
        query {
            user: actor {
                ... on User {
                    ...
                }
            }
        }
    "#;
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: updated_text.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let position = Position::new(8, 23); // after the inner "..."
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

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array"),
    };

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "ActorFields"),
        "Should suggest ActorFields inside User (member of union). Found: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields inside User. Found: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "GuestFields"),
        "Should NOT suggest GuestFields inside User. Found: {:?}",
        labels
    );
}
