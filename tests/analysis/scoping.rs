use crate::support::{
    completion_items_array, create_service, lsp_did_open, lsp_initialize_sequence,
    lsp_request_completion, lsp_request_typed, pos,
};
use graphql_rust::Config;
use std::path::Path;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_lsp_fragment_scoping() {
    let (mut service, _handle) = create_service(Config::new_empty());
    lsp_initialize_sequence(&mut service).await;

    // 1. Open Public/Private fragments in pkg_a
    let pkg_a_frag_path = std::fs::canonicalize(Path::new(
        "tests/fixtures/public_test/pkg_a/fragment.graphql",
    ))
    .unwrap();
    let pkg_a_uri = Url::from_file_path(pkg_a_frag_path).unwrap();
    let pkg_a_text = std::fs::read_to_string(pkg_a_uri.to_file_path().unwrap()).unwrap();

    lsp_did_open(&mut service, pkg_a_uri.clone(), "graphql", 1, &pkg_a_text).await;

    // 2. Open Query in pkg_b
    let pkg_b_query_path =
        std::fs::canonicalize(Path::new("tests/fixtures/public_test/pkg_b/query.graphql")).unwrap();
    let pkg_b_uri = Url::from_file_path(pkg_b_query_path).unwrap();
    let pkg_b_text = r#"
        query {
            users {
                ...PublicFrag
                ...PrivateFrag
            }
        }
    "#;

    lsp_did_open(
        &mut service,
        pkg_b_uri.clone(),
        "graphql",
        1,
        pkg_b_text,
    ).await;

    // 3. Request completions at "...P" in pkg_b
    // Line 4: "                ...PublicFrag"
    let result = lsp_request_completion(&mut service, pkg_b_uri.clone(), pos(3, 19)).await;
    let items = completion_items_array(&result);

    let has_public = items.iter().any(|i| i.label == "PublicFrag");
    let has_private = items.iter().any(|i| i.label == "PrivateFrag");

    assert!(has_public, "Should suggest PublicFrag from pkg_a to pkg_b");
    assert!(
        !has_private,
        "Should NOT suggest PrivateFrag from pkg_a to pkg_b"
    );

    // 4. Verify Go-to-Definition for PublicFrag in pkg_b
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: pkg_b_uri.clone(),
            },
            position: pos(3, 20),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert_eq!(loc.uri, pkg_a_uri, "Definition should point to pkg_a");
        }
        _ => panic!("Expected definition to be found in pkg_a, got {:?}", result),
    }
}

#[tokio::test]
async fn test_lsp_package_isolation() {
    let (mut service, _handle) = create_service(Config::new_empty());
    lsp_initialize_sequence(&mut service).await;

    // 1. Open pkg_a/fragment.graphql (defines FragmentA)
    let pkg_a_frag_path =
        std::fs::canonicalize(Path::new("tests/fixtures/scoped/pkg_a/fragment.graphql")).unwrap();
    let pkg_a_uri = Url::from_file_path(pkg_a_frag_path).unwrap();
    let pkg_a_text = "fragment FragmentA on User { id }";

    lsp_did_open(&mut service, pkg_a_uri.clone(), "graphql", 1, pkg_a_text).await;

    // 2. Open pkg_b/fragment.graphql (defines FragmentB)
    let pkg_b_frag_path =
        std::fs::canonicalize(Path::new("tests/fixtures/scoped/pkg_b/fragment.graphql")).unwrap();
    let pkg_b_uri = Url::from_file_path(pkg_b_frag_path).unwrap();
    let pkg_b_text = "fragment FragmentB on User { id }";

    lsp_did_open(&mut service, pkg_b_uri.clone(), "graphql", 1, pkg_b_text).await;

    // 3. Open pkg_b/query.graphql (attempts to spread FragmentA)
    let pkg_b_query_path =
        std::fs::canonicalize(Path::new("tests/fixtures/scoped/pkg_b/query.graphql")).unwrap();
    let pkg_b_query_uri = Url::from_file_path(pkg_b_query_path).unwrap();
    let pkg_b_query_text = "query { users { ...FragmentA } }";

    lsp_did_open(
        &mut service,
        pkg_b_query_uri.clone(),
        "graphql",
        1,
        pkg_b_query_text,
    ).await;

    // 4. Goto Definition for FragmentA in pkg_b/query.graphql
    // FragmentA should NOT be found because it's in pkg_a and not @public
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: pkg_b_query_uri.clone(),
            },
            position: pos(0, 20),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(result.is_none(), "FragmentA should not be visible in pkg_b");
}