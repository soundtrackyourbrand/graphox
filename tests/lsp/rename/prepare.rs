use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_prepare_rename_fragment() {
    let schema = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("fragment User|Fields on User { id name }");
    let uri = write_project_file(&dir, "fragment.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    };
    let result: Option<PrepareRenameResponse> =
        lsp_request_typed(&mut service, "textDocument/prepareRename", &params).await;
    assert!(
        result.is_some(),
        "Expected Some for prepareRename on fragment"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_prepare_rename_variable() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("query($|id: ID!) { user(id: $id) }");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    };
    let result: Option<PrepareRenameResponse> =
        lsp_request_typed(&mut service, "textDocument/prepareRename", &params).await;
    assert!(
        result.is_some(),
        "Expected Some for prepareRename on variable"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_prepare_rename_no_symbol() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("query { | }");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    };
    let result: Option<PrepareRenameResponse> =
        lsp_request_typed(&mut service, "textDocument/prepareRename", &params).await;
    assert!(
        result.is_none(),
        "Expected None for prepareRename with no symbol"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_prepare_rename_literal() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("query { user(id: \"||\") }");
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    };
    let result: Option<PrepareRenameResponse> =
        lsp_request_typed(&mut service, "textDocument/prepareRename", &params).await;
    assert!(
        result.is_none(),
        "Expected None for prepareRename on literal"
    );
}
