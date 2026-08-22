use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_directive_usage_rename() {
    let schema = "directive @skip(if: Boolean!) on FIELD type Query { user: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("query { user { id @|skip(if: true) } }");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "skipIf".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for directive usage rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_directive_definition_rename() {
    let schema =
        "directive @skip(if: Boolean!) on FIELD type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("directive @|skip(if: Boolean!) on FIELD");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "skipIf".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(
        result.is_some(),
        "Expected Some for directive definition rename"
    );
}
