use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help() {
    let schema = "type Query { me(id: ID, name: String): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { me(id: \"123\", |) }");
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
    assert_eq!(help.signatures[0].label, "me(id: ID, name: String)");
    assert_eq!(help.active_parameter, Some(1)); // Should be on "name"
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_tsx() {
    let schema = "type Query { me(id: ID, name: String): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (tsx_text, position) = with_cursor("const q = gql`query { me(id: \"123\", |) }`;");
    let tsx_uri = write_project_file(&dir, "Component.tsx", &tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text,
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

    let help = result.expect("Expected SignatureHelp in TSX");
    assert_eq!(help.signatures[0].label, "me(id: ID, name: String)");
    assert_eq!(help.active_parameter, Some(1));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_signature_help_with_alias() {
    let schema = "type Query { user(id: ID!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    // Use an alias for the field
    let (query_text, position) = with_cursor("query { myAlias: user(id:|) }");
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
