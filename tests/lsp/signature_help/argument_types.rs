use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_input_object_arg() {
    let schema = "input CreateUserInput { name: String! email: String } type Query { create(input: CreateUserInput!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { create(input: |) }");
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
    assert_eq!(help.signatures[0].label, "create(input: CreateUserInput!)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_nested_input_field() {
    let schema = "input AddressInput { street: String! city: String! } input UserInput { name: String! address: AddressInput } type Query { createUser(input: UserInput!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) =
        with_cursor("query { createUser(input: { name: \"John\", address: { | } }) }");
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
    assert_eq!(help.signatures[0].label, "createUser(input: UserInput!)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_enum_arg() {
    let schema = "enum Role { ADMIN USER MODERATOR } type Query { users(role: Role): [String] }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { users(role: |) }");
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
    assert_eq!(help.signatures[0].label, "users(role: Role)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_list_arg() {
    let schema = "type Query { users(ids: [ID!]!): [String] }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { users(ids: |) }");
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
    assert_eq!(help.signatures[0].label, "users(ids: [ID!]!)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_non_null_arg() {
    let schema = "type Query { user(id: ID!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user(id: |) }");
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
    assert_eq!(help.signatures[0].label, "user(id: ID!)");
}
