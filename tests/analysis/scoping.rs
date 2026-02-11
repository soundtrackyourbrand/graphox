use crate::support::{
    completion_items_array, create_initialized_lsp_service, create_service, lsp_did_open,
    lsp_initialize_sequence, lsp_request_completion, lsp_request_typed, with_cursor,
};
use graphox::Config;
use std::path::Path;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_lsp_fragment_scoping() {
    let workspace = crate::support::TestWorkspace::new();
    workspace.copy_from("tests/fixtures/public_test");

    let config = graphox::Config::load_from_dir(workspace.root())
        .expect("Failed to load config from workspace")
        .expect("Config should exist");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open Public/Private fragments in pkg_a
    let pkg_a_uri = workspace.uri_for("pkg_a/fragment.graphql");
    let pkg_a_text = std::fs::read_to_string(pkg_a_uri.to_file_path().unwrap()).unwrap();

    lsp_did_open(&mut service, pkg_a_uri.clone(), "graphql", 1, &pkg_a_text).await;

    // 2. Open Query in pkg_b
    let pkg_b_uri = workspace.uri_for("pkg_b/query.graphql");
    let (pkg_b_text, cursor_pos) = with_cursor(
        r#"
        query {
            users {
                ...PublicFrag
                ...P|rivateFrag
            }
        }
    "#,
    );

    lsp_did_open(&mut service, pkg_b_uri.clone(), "graphql", 1, &pkg_b_text).await;

    // 3. Request completions at "...P" in pkg_b
    let result = lsp_request_completion(&mut service, pkg_b_uri.clone(), cursor_pos).await;
    let items = completion_items_array(&result);

    let has_public = items.iter().any(|i| i.label == "PublicFrag");
    let has_private = items.iter().any(|i| i.label == "PrivateFrag");

    assert!(has_public, "Should suggest PublicFrag from pkg_a to pkg_b");
    assert!(
        !has_private,
        "Should NOT suggest PrivateFrag from pkg_a to pkg_b"
    );

    // 4. Verify Go-to-Definition for PublicFrag in pkg_b
    let (pkg_b_text2, cursor_pos2) = with_cursor(
        r#"
        query {
            users {
                ...Public|Frag
                ...PrivateFrag
            }
        }
    "#,
    );
    workspace.write_file("pkg_b/query2.graphql", &pkg_b_text2);
    let pkg_b_uri2 = workspace.uri_for("pkg_b/query2.graphql");
    lsp_did_open(&mut service, pkg_b_uri2.clone(), "graphql", 1, &pkg_b_text2).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: pkg_b_uri2.clone(),
            },
            position: cursor_pos2,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            // Note: pkg_a_uri is in the temp workspace now
            assert_eq!(
                loc.uri, pkg_a_uri,
                "Definition should point to pkg_a in workspace"
            );
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
    let (pkg_b_query_text, cursor_pos) = with_cursor("query { users { ...Frag|mentA } }");
    let pkg_b_query_path =
        std::fs::canonicalize(Path::new("tests/fixtures/scoped/pkg_b/query.graphql")).unwrap();
    let pkg_b_query_uri = Url::from_file_path(pkg_b_query_path).unwrap();

    lsp_did_open(
        &mut service,
        pkg_b_query_uri.clone(),
        "graphql",
        1,
        &pkg_b_query_text,
    )
    .await;

    // 4. Goto Definition for FragmentA in pkg_b/query.graphql
    // FragmentA should NOT be found because it's in pkg_a and not @public
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: pkg_b_query_uri.clone(),
            },
            position: cursor_pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(result.is_none(), "FragmentA should not be visible in pkg_b");
}
