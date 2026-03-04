use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_builtin_type() {
    // Built-in scalars cannot be renamed
    let schema = "type Query { user: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("type Query { user: |String }");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "Str".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    // Built-in types typically can't be renamed - expect None or empty
    assert!(
        result.is_none()
            || result
                .as_ref()
                .map(|r| r.changes.as_ref().map(|c| c.is_empty()).unwrap_or(true))
                .unwrap_or(true)
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_schema_keyword() {
    // Keywords cannot be renamed
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("|query { user { id } }");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "myQuery".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    // Keywords cannot be renamed - expect None or empty
    assert!(
        result.is_none()
            || result
                .as_ref()
                .map(|r| r.changes.as_ref().map(|c| c.is_empty()).unwrap_or(true))
                .unwrap_or(true)
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_in_comment() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("# comment with | id");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "renamed".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    // Comments cannot be renamed
    assert!(
        result.is_none()
            || result
                .as_ref()
                .map(|r| r.changes.as_ref().map(|c| c.is_empty()).unwrap_or(true))
                .unwrap_or(true)
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_no_changes() {
    // Rename symbol that isn't used anywhere else
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("fragment User|Fields on User { id }");
    let uri = write_project_file(&dir, "fragment.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        new_name: "RenamedUserFields".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    // Should still return edit for the single file even if no other files reference it
    assert!(result.is_some());
    let edit = result.unwrap();
    assert!(edit.changes.is_some());
    let changes = edit.changes.unwrap();
    assert!(changes.contains_key(&uri));
}
