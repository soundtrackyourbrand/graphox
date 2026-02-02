use std::time::Duration;
use tower_lsp::jsonrpc::{Request, Response};
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};
use graphql_rust::Backend;
use tower_service::Service;

#[tokio::test]
async fn test_goto_definition_cross_file() {
    let (mut service, _) = LspService::new(|client| Backend::new(client, "tests/fixtures/simple_schema.graphql"));

    // 0. Initialize
    let init_params = InitializeParams { ..Default::default() };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();
    
    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // 1. Open the fragment definition file
    let fragment_uri = Url::parse("file:///tests/fixtures/fragments/user_fragment.ts").unwrap();
    let fragment_text = std::fs::read_to_string("tests/fixtures/fragments/user_fragment.ts").unwrap();
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: fragment_uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: fragment_text,
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // 2. Open the query file that uses the fragment
    let query_uri = Url::parse("file:///tests/fixtures/fragments/query_with_fragment.ts").unwrap();
    let query_text = std::fs::read_to_string("tests/fixtures/fragments/query_with_fragment.ts").unwrap();
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: query_text,
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // 3. Trigger Go to Definition on "...UserFragment" in query file
    let position = Position::new(6, 10); 
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    
    // Request ID 1
    let request = Request::build("textDocument/definition")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
        
    let response = service.call(request).await.unwrap().unwrap();
    
    if let Some(err) = response.error() {
        panic!("JSON-RPC Error: {:?}", err);
    }

    let result: Option<GotoDefinitionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, fragment_uri);
            // Line 3:   fragment UserFragment on User {
            // 2 spaces + "fragment " (9) = 11.
            assert_eq!(location.range.start.line, 3);
            assert_eq!(location.range.start.character, 11);
        }
        _ => panic!("Expected Scalar location, got {:?}", result),
    }
}
