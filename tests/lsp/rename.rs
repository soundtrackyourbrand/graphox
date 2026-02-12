use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_fragment_rename() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Fragment file
    let (fragment_text, position) = with_cursor("fragment User|Fields on User { id name }");
    let fragment_uri = write_project_file(&tmpdir, "user_fragment.graphql", &fragment_text);
    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        &fragment_text,
    )
    .await;

    // 2. Query file
    let query_text = "query { user { ...UserFields } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Rename on "UserFields" in fragment file to "MyFields"
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "MyFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);

    let frag_edits = &changes[&fragment_uri];
    assert_eq!(frag_edits.len(), 1);
    assert_eq!(frag_edits[0].new_text, "MyFields");
    assert_eq!(frag_edits[0].range.start.character, 9);

    let query_edits = &changes[&query_uri];
    assert_eq!(query_edits.len(), 1);
    assert_eq!(query_edits[0].new_text, "MyFields");
    assert_eq!(query_edits[0].range.start.character, 18);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_fragment_rename_tsx() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.{graphql,tsx}",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Fragment file
    let (fragment_text, position) = with_cursor("fragment User|Fields on User { id name }");
    let fragment_uri = write_project_file(&tmpdir, "user_fragment.graphql", &fragment_text);
    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        &fragment_text,
    )
    .await;

    // 2. TSX file
    let tsx_text = r#"
        const query = gql`
            query {
                user {
                    ...UserFields
                }
            }
        `;
    "#;
    let tsx_uri = write_project_file(&tmpdir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    // 3. Trigger Rename on "UserFields" in fragment file
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "RenamedFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);

    let tsx_edits = &changes[&tsx_uri];
    assert_eq!(tsx_edits.len(), 1);
    assert_eq!(tsx_edits[0].new_text, "RenamedFields");

    // Verify it correctly identified the location in TSX
    let line = tsx_text
        .lines()
        .nth(tsx_edits[0].range.start.line as usize)
        .unwrap();
    assert!(line.contains("...UserFields"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_unopened_file() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );

    // Write the query file into the workspace BEFORE initializing the LSP service so the
    // workspace scan discovers it. The test expects an unopened file to be included in
    // the rename `WorkspaceEdit`.
    let query_text = "query { user { ...UserFields } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open only the fragment file
    let (fragment_text, position) = with_cursor("fragment User|Fields on User { id name }");
    let fragment_uri = write_project_file(&tmpdir, "user_fragment.graphql", &fragment_text);
    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        &fragment_text,
    )
    .await;

    // Trigger Rename on "UserFields" in fragment file to "MyFields"
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "MyFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    // Check if both files are present in the changes
    assert!(
        changes.contains_key(&fragment_uri),
        "Changes should include fragment file"
    );
    assert!(
        changes.contains_key(&query_uri),
        "Changes should include unopened query file"
    );

    let frag_edits = &changes[&fragment_uri];
    assert_eq!(frag_edits.len(), 1);
    assert_eq!(frag_edits[0].new_text, "MyFields");

    let query_edits = &changes[&query_uri];
    assert_eq!(query_edits.len(), 1);
    assert_eq!(query_edits[0].new_text, "MyFields");
    assert_eq!(query_edits[0].range.start.character, 18);
}
