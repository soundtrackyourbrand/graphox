use graphox::{Backend, Config};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

fn with_cursor(text: &str) -> (String, Position) {
    crate::support::with_cursor(text)
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
        projects: vec![graphox::config::ProjectConfig {
            schema: graphox::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphox::config::GlobPattern::Single("test.graphql".to_string()),
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

    let query_path = dir.path().join("test.graphql");
    let (text, position) = with_cursor("query { | }");
    fs::write(&query_path, &text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.clone(),
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
        assert!(
            items.iter().any(|i| i.label == "users"),
            "Should include regular field 'users'"
        );

        assert!(
            items.iter().any(|i| i.label == "__typename"),
            "Should include __typename"
        );

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

    let (text2, position2) = with_cursor("query { users { | } }");
    fs::write(&query_path, &text2).unwrap();

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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: position2,
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
        assert!(
            items.iter().any(|i| i.label == "id"),
            "Should include regular field 'id'"
        );
        assert!(
            items.iter().any(|i| i.label == "username"),
            "Should include regular field 'username'"
        );

        assert!(
            items.iter().any(|i| i.label == "__typename"),
            "Should include __typename on User type"
        );

        assert!(
            !items.iter().any(|i| i.label == "__schema"),
            "Should NOT include __schema on non-root User type"
        );

        assert!(
            !items.iter().any(|i| i.label == "__type"),
            "Should NOT include __type on non-root User type"
        );
    } else {
        panic!("Expected array of completions");
    }
}
