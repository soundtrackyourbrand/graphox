use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_introspection_fields() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { | }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;

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
    crate::support::lsp_send_notification(
        &mut service,
        "textDocument/didChange",
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text2.to_string(),
            }],
        },
    )
    .await;

    let result = lsp_request_completion(&mut service, uri.clone(), position2).await;

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
