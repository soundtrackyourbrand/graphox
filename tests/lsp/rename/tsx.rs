use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_variable_tsx() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("const q = gql`query($|id: ID!) { user(id: $id) }`;");
    let uri = write_project_file(&dir, "Component.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescriptreact", 1, &text).await;

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
    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");
    assert!(!changes.is_empty(), "Expected changes in TSX file");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_fragment_in_tsx() {
    let schema = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    // Fragment definition in TSX
    let tsx_text = "const q = gql`fragment User|Fields on User { id name }`;";
    let uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(&mut service, uri.clone(), "typescriptreact", 1, tsx_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(0, 26),
        },
        new_name: "UserDetails".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for fragment rename in TSX");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_field_in_tsx() {
    let schema = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (tsx_text, position) = with_cursor("const q = gql`query { user { |name } }`;");
    let uri = write_project_file(&dir, "Component.tsx", &tsx_text);
    lsp_did_open(&mut service, uri.clone(), "typescriptreact", 1, &tsx_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "userName".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for field rename in TSX");
}
