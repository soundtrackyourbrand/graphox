use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_rename() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User } type User { id: ID! }",
        "**/*.graphql",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query($|id: ID!) { user(id: $id) }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        new_name: "userId".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 1);
    let edits = &changes[&query_uri];
    assert_eq!(edits.len(), 2);

    let mut new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    new_texts.sort();
    assert_eq!(new_texts, vec!["userId", "userId"]);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_rename_definition_and_usage() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user(id: ID!, name: String): User } type User { id: ID! }",
        "**/*.graphql",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query($|id: ID!) { user(id: $id) }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        new_name: "userId".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 1);
    let edits = &changes[&query_uri];
    assert_eq!(edits.len(), 2);

    let mut new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    new_texts.sort();
    assert_eq!(new_texts, vec!["userId", "userId"]);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_rename_multiple_usages() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User other: User } type User { id: ID! }",
        "**/*.graphql",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query($|id: ID!) { user(id: $id) other(id: $id) }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        new_name: "userId".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 1);
    let edits = &changes[&query_uri];
    assert_eq!(edits.len(), 3);

    let mut new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    new_texts.sort();
    assert_eq!(new_texts, vec!["userId", "userId", "userId"]);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_rename_fragment() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User } type User { id: ID! }",
        "**/*.graphql",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let fragment_text = "fragment UserFrag on User { id }";
    let fragment_uri = write_project_file(&tmpdir, "user_fragment.graphql", fragment_text);
    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        fragment_text,
    )
    .await;

    let (query_text, position) = with_cursor("query($|id: ID!) { user(id: $id) ...UserFrag }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        new_name: "userId".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 1);
    let edits = &changes[&query_uri];
    assert_eq!(edits.len(), 2);

    let mut new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    new_texts.sort();
    assert_eq!(new_texts, vec!["userId", "userId"]);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_rename_tsx() {
    let (tmpdir, config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User } type User { id: ID! }",
        "**/*.{graphql,tsx}",
    );

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (tsx_text, position) = with_cursor(r#"const q = gql`query($|id: ID!) { user(id: $id) }`;"#);
    let tsx_uri = write_project_file(&tmpdir, "Component.tsx", &tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text,
    )
    .await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
            },
            position,
        },
        new_name: "userId".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 1);
    let edits = &changes[&tsx_uri];
    assert_eq!(edits.len(), 2);

    let mut new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    new_texts.sort();
    assert_eq!(new_texts, vec!["userId", "userId"]);
}
