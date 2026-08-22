use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_argument_rename() {
    let schema = "type Query { user(id: ID!, name: String): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("type Query { user(|id: ID!, name: String): User }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "userId".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for argument rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_input_field_rename() {
    let schema = "input UserInput { name: String! email: String } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("input UserInput { |name: String! email: String }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "fullName".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for input field rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_enum_value_rename() {
    let schema = "enum Role { ADMIN USER MODERATOR } type Query { role: Role }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("enum Role { |ADMIN USER MODERATOR }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "SUPER_ADMIN".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for enum value rename");
}
