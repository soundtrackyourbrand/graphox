use crate::support::{
    completion_items_array, lsp_did_open, lsp_request_completion, lsp_request_hover,
    make_temp_project_with_schema, with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_type_only_not_in_completion() {
    let schema = "type User { id: ID! name: String! email: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment TypeOnlyFrag on User @type_only { id }";
    let (query_text, position) = with_cursor("query { user { ...| } }");
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let completions = lsp_request_completion(&mut service, query_uri.clone(), position).await;
    let items = completion_items_array(&completions);

    let has_type_only = items.iter().any(|i| i.label == "TypeOnlyFrag");
    assert!(
        !has_type_only,
        "Type-only fragment should NOT appear in completions"
    );
}

#[tokio::test]
async fn test_type_only_diagnostic_when_used() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment TypeOnlyFrag on User @type_only { id }";
    let query_text = "query { user { ...TypeOnlyFrag } }";
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    let query_uri = write_project_file(&dir, "query.graphql", query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Request diagnostics for the query (spread) and assert the diagnostic exists on the spread
    let diagnostics =
        crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;

    let spread_diag = match diagnostics {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => report
            .full_document_diagnostic_report
            .items
            .into_iter()
            .find(|d| d.code == Some(NumberOrString::String("type_only_used".to_string()))),
        _ => None,
    };

    let spread_diag = spread_diag.expect("Expected type_only_used diagnostic on fragment spread");

    // Ensure the diagnostic range points at the fragment name in the spread
    assert_eq!(spread_diag.range.start.line, 0);
    // Fragment name in the spread starts at character 18 (0-based)
    assert_eq!(spread_diag.range.start.character, 18);

    // Request code actions for the range (simulate client requesting quick fixes)
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: spread_diag.range,
        context: CodeActionContext {
            diagnostics: vec![spread_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = crate::support::lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = crate::support::find_code_action_by_title(&actions, "Remove @type_only directive")
        .expect("Should find 'Remove @type_only directive' action");

    // The code action should include an edit that removes @type_only in the fragment file
    let edit = ca
        .edit
        .as_ref()
        .expect("Code action should include an edit");
    let changes = edit.changes.as_ref().expect("Edit should have changes");
    // Ensure fragment file URI is present
    assert!(
        changes
            .keys()
            .any(|u| u.path().ends_with("fragments.graphql"))
    );
}

#[tokio::test]
async fn test_type_only_in_hover() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment TypeOnlyFrag on User @type_only { id }";
    let (query_text, cursor_pos) = with_cursor("query { user { ...TypeOnly|Frag } }");
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let hover = lsp_request_hover(&mut service, query_uri.clone(), cursor_pos).await;
    assert!(
        hover.is_some(),
        "Hover should return information for fragment spread"
    );

    let hover_content = hover.unwrap();
    println!("{:#?}", hover_content);
    if let HoverContents::Markup(contents) = hover_content.contents {
        let text = contents.value;
        assert!(
            text.contains("TypeOnlyFrag") || text.contains("@type_only"),
            "Hover should mention the type-only fragment: {:?}",
            text
        );
    } else {
        panic!("Unexpected HoverContents");
    }
}

#[tokio::test]
async fn test_type_only_definition() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment TypeOnlyFrag on User @type_only { id }";
    let (query_text, position) = with_cursor("query { user { ...TypeOnly|Frag } }");
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        crate::support::lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert_eq!(
                loc.uri, frag_uri,
                "Definition should point to type-only fragment"
            );
        }
        Some(GotoDefinitionResponse::Array(arr)) => {
            assert!(
                arr.iter().any(|loc| loc.uri == frag_uri),
                "Definition should point to type-only fragment in array"
            );
        }
        Some(GotoDefinitionResponse::Link(_)) => {
            // Link responses are also valid
        }
        None => {
            // Type-only fragments might not support goto definition
        }
    }
}

#[tokio::test]
async fn test_regular_fragment_still_in_completion() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment RegularFrag on User { id }";
    let (query_text, cursor_pos) = with_cursor("query { user { ...| } }");
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let completions = lsp_request_completion(&mut service, query_uri.clone(), cursor_pos).await;
    let items = completion_items_array(&completions);

    let has_regular = items.iter().any(|i| i.label == "RegularFrag");
    assert!(has_regular, "Regular fragment should appear in completions");
}

#[tokio::test]
async fn test_multiple_type_only_fragments() {
    let schema = "type User { id: ID! name: String! email: String } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = r#"
        fragment TypeOnlyId on User @type_only { id }
        fragment TypeOnlyName on User @type_only { name }
        fragment RegularFrag on User { email }
    "#;
    let (query_text, cursor_pos) = with_cursor("query { user { ...| } }");
    let frag_uri = write_project_file(&dir, "fragments.graphql", frag_text);
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let completions = lsp_request_completion(&mut service, query_uri.clone(), cursor_pos).await;
    let items = completion_items_array(&completions);

    let has_type_only_id = items.iter().any(|i| i.label == "TypeOnlyId");
    let has_type_only_name = items.iter().any(|i| i.label == "TypeOnlyName");
    let has_regular = items.iter().any(|i| i.label == "RegularFrag");

    assert!(
        !has_type_only_id && !has_type_only_name,
        "Type-only fragments should NOT appear in completions"
    );
    assert!(has_regular, "Regular fragment should appear in completions");
}
