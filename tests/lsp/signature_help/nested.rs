use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_nested_field() {
    let schema = "type Query { user: User } type User { posts(limit: Int): [Post] } type Post { id: ID! title: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user { posts(limit: |) } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let result: Option<SignatureHelp> =
        lsp_request_typed(&mut service, "textDocument/signatureHelp", &params).await;

    let help = result.expect("Expected SignatureHelp");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "posts(limit: Int)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_deeply_nested() {
    let schema = "type Query { user: User } type User { posts: [Post] } type Post { comments(limit: Int): [Comment] } type Comment { id: ID! text: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user { posts { comments(limit: |) } } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let result: Option<SignatureHelp> =
        lsp_request_typed(&mut service, "textDocument/signatureHelp", &params).await;

    let help = result.expect("Expected SignatureHelp");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "comments(limit: Int)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_sibling_fields() {
    let schema =
        "type Query { user: User } type User { id: ID! name: String posts(limit: Int): [String] }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user { id name posts(limit: |) } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let result: Option<SignatureHelp> =
        lsp_request_typed(&mut service, "textDocument/signatureHelp", &params).await;

    let help = result.expect("Expected SignatureHelp");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "posts(limit: Int)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_interface_field() {
    let schema = "interface Node { id: ID! } type User implements Node { id: ID! name: String } type Query { node(id: ID!): Node }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { node(id: |) }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let result: Option<SignatureHelp> =
        lsp_request_typed(&mut service, "textDocument/signatureHelp", &params).await;

    let help = result.expect("Expected SignatureHelp");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "node(id: ID!)");
}
