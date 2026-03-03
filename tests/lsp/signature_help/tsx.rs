use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_tsx_mutation() {
    let schema = "type Mutation { createUser(name: String!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = "const q = gql`mutation { createUser(name: |) }`;";
    let (tsx_text_with_cursor, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&dir, "Component.tsx", &tsx_text_with_cursor);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text_with_cursor,
    )
    .await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
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
async fn test_signature_help_tsx_subscription() {
    let schema = "type Subscription { onEvent(id: ID!): Event } type Event { id: ID! } type Query { me: Event }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = "const q = gql`subscription { onEvent(id: |) }`;";
    let (tsx_text_with_cursor, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&dir, "Component.tsx", &tsx_text_with_cursor);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text_with_cursor,
    )
    .await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
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
    assert_eq!(help.signatures[0].label, "onEvent(id: ID!)");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_tsx_nested() {
    let schema =
        "type Query { user: User } type User { posts(limit: Int): [Post] } type Post { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = "const q = gql`query { user { posts(limit: |) } }`;";
    let (tsx_text_with_cursor, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&dir, "Component.tsx", &tsx_text_with_cursor);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text_with_cursor,
    )
    .await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
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
async fn test_signature_help_tsx_input_object() {
    let schema = "input CreateUserInput { name: String! } type Mutation { createUser(input: CreateUserInput!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = "const q = gql`mutation { createUser(input: |) }`;";
    let (tsx_text_with_cursor, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&dir, "Component.tsx", &tsx_text_with_cursor);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text_with_cursor,
    )
    .await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
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
async fn test_signature_help_tsx_multiple_blocks() {
    let schema = "type Query { user: User } type Mutation { updateUser(name: String!): User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text =
        "const q1 = gql`query { user } `; const q2 = gql`mutation { updateUser(name: |) }`;";
    let (tsx_text_with_cursor, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&dir, "Component.tsx", &tsx_text_with_cursor);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text_with_cursor,
    )
    .await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
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
    assert_eq!(help.signatures[0].label, "updateUser(name: String!)");
}
