use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_extract_single_field() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let range = crate::support::range_for_token(&doc, query_text, "id");
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range,
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find 'Extract to fragment' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&query_uri];

    assert_eq!(edits.len(), 2);
    assert!(
        edits
            .iter()
            .any(|e| e.new_text.contains("fragment NewFragment"))
    );
    assert!(edits.iter().any(|e| e.new_text.contains("...NewFragment")));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_extract_interface_type() {
    let schema = "interface Node { id: ID! } type User implements Node { id: ID! name: String } type Query { me: Node }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let range = crate::support::range_for_token(&doc, query_text, "id");
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range,
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find 'Extract to fragment' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&query_uri];

    assert_eq!(edits.len(), 2);
    assert!(
        edits
            .iter()
            .any(|e| e.new_text.contains("fragment NewFragment on Node"))
    );
}
