use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_rename_aliased() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user { u: |name } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        new_name: "fullName".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    if let Some(edit) = result
        && let Some(changes) = edit.changes
        && let Some(edits) = changes.get(&query_uri)
    {
        for text_edit in edits {
            assert!(
                !text_edit.new_text.contains("u:"),
                "Alias 'u' should not be renamed"
            );
        }
    }
}
