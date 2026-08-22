use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_mutation() {
    let schema = "type Mutation { createUser(name: String!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("mutation { createUser(name:|) }");
    let query_uri = write_project_file(&dir, "mutation.graphql", &query_text);
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
    assert_eq!(help.signatures[0].label, "createUser(name: String!)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_mutation_multiple_args() {
    let schema = "type Mutation { createUser(name: String!, email: String): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("mutation { createUser(name: \"test\", |) }");
    let query_uri = write_project_file(&dir, "mutation.graphql", &query_text);
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
    assert_eq!(
        help.signatures[0].label,
        "createUser(name: String!, email: String)"
    );
    assert_eq!(help.active_parameter, Some(1)); // Should be on "email"
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_mutation_input_object() {
    let schema = "input CreateUserInput { name: String! email: String } type Mutation { createUser(input: CreateUserInput!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("mutation { createUser(input: |) }");
    let query_uri = write_project_file(&dir, "mutation.graphql", &query_text);
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
    assert_eq!(
        help.signatures[0].label,
        "createUser(input: CreateUserInput!)"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_subscription() {
    let schema = "type Subscription { userUpdated(id: ID!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("subscription { userUpdated(id:|) }");
    let query_uri = write_project_file(&dir, "subscription.graphql", &query_text);
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
    assert_eq!(help.signatures[0].label, "userUpdated(id: ID!)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_subscription_args() {
    let schema = "type Subscription { onMessage(channel: String!, limit: Int): Message } type Message { id: ID! text: String } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("subscription { onMessage(channel: \"general\", |) }");
    let query_uri = write_project_file(&dir, "subscription.graphql", &query_text);
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
    assert_eq!(
        help.signatures[0].label,
        "onMessage(channel: String!, limit: Int)"
    );
    assert_eq!(help.active_parameter, Some(1)); // Should be on "limit"
}
