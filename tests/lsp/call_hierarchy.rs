use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    pos, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_fragment_call_hierarchy() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let frag_text = r#"
        fragment UserFields on User { id name }
        fragment UserWithHome on User { ...UserFields }
    "#;
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // 1. Prepare Call Hierarchy on "UserFields"
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: pos(1, 17), // UserFields definition
        },
        work_done_progress_params: Default::default(),
    };

    let result: Option<Vec<CallHierarchyItem>> =
        lsp_request_typed(&mut service, "textDocument/prepareCallHierarchy", &params).await;

    let items = result.expect("Expected CallHierarchyItems");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.name, "UserFields");

    // 2. Test Incoming Calls (who calls UserFields?)
    let params = CallHierarchyIncomingCallsParams {
        item: item.clone(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<Vec<CallHierarchyIncomingCall>> =
        lsp_request_typed(&mut service, "callHierarchy/incomingCalls", &params).await;

    let calls = result.expect("Expected incoming calls");
    assert!(!calls.is_empty());
    assert!(calls.iter().any(|c| c.from.name == "UserWithHome"));

    // 3. Test Outgoing Calls for "UserWithHome"
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: pos(2, 17), // UserWithHome definition
        },
        work_done_progress_params: Default::default(),
    };
    let result: Vec<CallHierarchyItem> =
        lsp_request_typed(&mut service, "textDocument/prepareCallHierarchy", &params).await;
    let item_with_home = &result[0];

    let params = CallHierarchyOutgoingCallsParams {
        item: item_with_home.clone(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result: Option<Vec<CallHierarchyOutgoingCall>> =
        lsp_request_typed(&mut service, "callHierarchy/outgoingCalls", &params).await;

    let calls = result.expect("Expected outgoing calls");
    assert!(!calls.is_empty());
    assert!(calls.iter().any(|c| c.to.name == "UserFields"));
}

#[tokio::test]
async fn test_call_hierarchy_tsx() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment UserFields on User { id }";
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let tsx_text = "const q = gql`query { me { ...UserFields } }`;";
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    // 1. Prepare Call Hierarchy on "UserFields"
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: pos(0, 9),
        },
        work_done_progress_params: Default::default(),
    };

    let result: Vec<CallHierarchyItem> =
        lsp_request_typed(&mut service, "textDocument/prepareCallHierarchy", &params).await;
    let item = &result[0];

    // 2. Test Incoming Calls (should show TSX query)
    let params = CallHierarchyIncomingCallsParams {
        item: item.clone(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<Vec<CallHierarchyIncomingCall>> =
        lsp_request_typed(&mut service, "callHierarchy/incomingCalls", &params).await;

    let calls = result.expect("Expected incoming calls");
    assert!(calls.iter().any(|c| c.from.uri == tsx_uri));
}
