use tower_lsp::lsp_types::{
    SignatureHelp, SignatureHelpParams, TextDocumentIdentifier, TextDocumentPositionParams,
};

use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_active_param_first() {
    let schema = "type Query { user(id: ID!, name: String): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user(id: \"1\", |) }");
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
    assert_eq!(help.active_parameter, Some(1));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_active_param_second() {
    let schema = "type Query { user(id: ID!, name: String, age: Int): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user(id: \"1\", name: \"John\", |) }");
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
    assert_eq!(help.active_parameter, Some(2));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_active_param_third() {
    let schema = "type Query { user(a: String, b: String, c: String): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user(a: \"1\", b: \"2\", c: \"3\", |) }");
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
    assert_eq!(help.active_parameter, None);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_active_param_empty() {
    let schema = "type Query { user(id: ID!, name: String): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user(|) }");
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

    assert!(result.is_none(), "Expected no SignatureHelp for empty args");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_active_param_after_all() {
    let schema = "type Query { user(id: ID!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user(id: \"1\")| }");
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
    assert_eq!(help.active_parameter, None);
}
