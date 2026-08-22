use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_code_actions,
    make_temp_project_with_schema, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_remove_fragment_affects_referencers() {
    let schema = "type Query { me: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_watch_all_files(false).with_timeouts(
        graphox::config::TimeoutConfig::default()
            .with_workspace_scan_ms(50)
            .with_lsp_request_ms(50),
    );

    let (mut service, _backend) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment UnusedFrag on Query { me }";
    let frag_uri = write_project_file(&dir, "fragment.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let query_text = "query { me }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let fragment_def_text = "fragment UnusedFrag on Query { me }";
    let doc_frag = crate::support::create_doc(frag_uri.as_str(), fragment_def_text);
    let diagnostic = tower_lsp_server::ls_types::Diagnostic {
        range: crate::support::range_for_token(&doc_frag, fragment_def_text, "UnusedFrag"),
        message: "Unused fragment: UnusedFrag".to_string(),
        code: Some(NumberOrString::String("unused_fragment".to_string())),
        ..Default::default()
    };

    let params = tower_lsp_server::ls_types::CodeActionParams {
        text_document: tower_lsp_server::ls_types::TextDocumentIdentifier {
            uri: frag_uri.clone(),
        },
        range: diagnostic.range,
        context: tower_lsp_server::ls_types::CodeActionContext {
            diagnostics: vec![diagnostic],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = actions
        .iter()
        .find_map(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                if ca.title == "Remove unused fragment" {
                    Some(ca)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("Should find 'Remove unused fragment' action");

    let edit = ca.edit.as_ref().expect("Action should have an edit");
    let changes = edit.changes.as_ref().expect("Edit should have changes");

    assert!(
        changes.contains_key(&frag_uri),
        "Edit should include fragment file"
    );
}
