use tower_lsp::lsp_types::*;
use graphql_rust::Backend;
use tower_lsp::LspService;
use tower_service::Service;
use tower_lsp::jsonrpc::Request;
use std::path::Path;

#[tokio::test]
async fn test_lsp_fragment_scoping() {
    let (mut service, _) = LspService::new(|client| Backend::new(client, None, "tests/fixtures/simple_schema.graphql"));

    // Initialize
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

    // 1. Open Public/Private fragments in pkg_a
    let pkg_a_frag_path = std::fs::canonicalize(Path::new("tests/fixtures/public_test/pkg_a/fragment.graphql")).unwrap();
    let pkg_a_uri = Url::from_file_path(pkg_a_frag_path).unwrap();
    let pkg_a_text = std::fs::read_to_string(&pkg_a_uri.to_file_path().unwrap()).unwrap();
    
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: pkg_a_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: pkg_a_text,
        },
    };
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

    // 2. Open Query in pkg_b
    let pkg_b_query_path = std::fs::canonicalize(Path::new("tests/fixtures/public_test/pkg_b/query.graphql")).unwrap();
    let pkg_b_uri = Url::from_file_path(pkg_b_query_path).unwrap();
    let pkg_b_text = r#"
        query {
            users {
                ...PublicFrag
                ...PrivateFrag
            }
        }
    "#;
    
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: pkg_b_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: pkg_b_text.to_string(),
        },
    };
    // didOpen triggers diagnostics, but we can also manually check completions
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

    // 3. Request completions at "...P" in pkg_b
    // Line 4: "                ...PublicFrag"
    // We'll put cursor after "..."
    let position = Position::new(3, 19); 
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: pkg_b_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let has_public = items.iter().any(|i| i.label == "PublicFrag");
        let has_private = items.iter().any(|i| i.label == "PrivateFrag");
        
        assert!(has_public, "Should suggest PublicFrag from pkg_a to pkg_b");
        assert!(!has_private, "Should NOT suggest PrivateFrag from pkg_a to pkg_b");
    } else {
        panic!("Expected array of completions");
    }

    // 4. Verify Go-to-Definition for PublicFrag in pkg_b
    let position = Position::new(3, 20); // on PublicFrag
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: pkg_b_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/definition")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<GotoDefinitionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert_eq!(loc.uri, pkg_a_uri, "Definition should point to pkg_a");
        }
        _ => panic!("Expected definition to be found in pkg_a, got {:?}", result),
    }
}

#[tokio::test]
async fn test_lsp_package_isolation() {
    let (mut service, _) = LspService::new(|client| {
        Backend::new(client, None, "tests/fixtures/simple_schema.graphql")
    });

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // 1. Open pkg_a/fragment.graphql (defines FragmentA)
    let pkg_a_frag_path = std::fs::canonicalize(Path::new(
        "tests/fixtures/scoped/pkg_a/fragment.graphql",
    ))
    .unwrap();
    let pkg_a_uri = Url::from_file_path(pkg_a_frag_path).unwrap();
    let pkg_a_text = "fragment FragmentA on User { id }";

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: pkg_a_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: pkg_a_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 2. Open pkg_b/fragment.graphql (defines FragmentB)
    let pkg_b_frag_path = std::fs::canonicalize(Path::new(
        "tests/fixtures/scoped/pkg_b/fragment.graphql",
    ))
    .unwrap();
    let pkg_b_uri = Url::from_file_path(pkg_b_frag_path).unwrap();
    let pkg_b_text = "fragment FragmentB on User { id }";

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: pkg_b_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: pkg_b_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 3. Open pkg_b/query.graphql (attempts to spread FragmentA)
    let pkg_b_query_path = std::fs::canonicalize(Path::new(
        "tests/fixtures/scoped/pkg_b/query.graphql",
    ))
    .unwrap();
    let pkg_b_query_uri = Url::from_file_path(pkg_b_query_path).unwrap();
    let pkg_b_query_text = "query { users { ...FragmentA } }";

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: pkg_b_query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: pkg_b_query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 4. Goto Definition for FragmentA in pkg_b/query.graphql
    // FragmentA should NOT be found because it's in pkg_a and not @public
    let position = Position::new(0, 20);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: pkg_b_query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/definition")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(
        result.is_none(),
        "FragmentA should not be visible in pkg_b"
    );
}
