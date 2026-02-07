use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, lsp_request_diagnostics, make_temp_project_with_schema, range,
    write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_missing_field_diagnostic_with_suggestions() {
    // Use helpers to create temp project, initialize service and open file
    let schema = "type Query { user: User } type User { id: ID! name: String } ";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Prepare query text and write it into the temp workspace so LSP can see it
    let text = "query { user { id nam } }";
    let query_uri = write_project_file(&dir, "query.graphql", text);
    // Simulate opening the document with the LSP
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, text).await;

    // Request diagnostics
    let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;

    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) = result {
        let diagnostics = &full_report.full_document_diagnostic_report.items;
        
        assert_eq!(diagnostics.len(), 1);
        let missing_field_diag = &diagnostics[0];
        
        assert!(missing_field_diag.message.contains("Field 'nam' not found"));
        assert!(missing_field_diag.message.contains("Did you mean 'name'"));

        // Verify the diagnostic has the correct code
        assert_eq!(
            missing_field_diag.code,
            Some(NumberOrString::String("missing_field".to_string()))
        );

        // Verify range
        let doc = create_doc(query_uri.as_str(), text);
        assert_eq!(missing_field_diag.range, crate::support::range_for_token(&doc, text, "nam"));

        // Verify data contains similar_fields
        if let Some(data) = &missing_field_diag.data {
            let similar_fields: Vec<String> = serde_json::from_value::<Vec<String>>(
                data.get("similar_fields").unwrap().clone(),
            )
            .unwrap();
            assert_eq!(similar_fields, vec!["name".to_string()]);
        } else {
            panic!("Diagnostic should have data with similar_fields");
        }
    } else {
        panic!("Expected full diagnostic report");
    }
}

#[tokio::test]
async fn test_missing_field_code_actions() {
    let schema = "type User { id: ID! name: String! email: String! username: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Typo: "usrname" instead of "username"
    let query_text = "query { user { id usrname } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Construct a diagnostic manually (in real scenario, this would come from diagnostics)
    let diagnostic = Diagnostic {
        range: range(0, 19, 0, 26), // "usrname"
        message: "Field 'usrname' not found on type 'User'. Did you mean 'username'?".to_string(),
        code: Some(NumberOrString::String("missing_field".to_string())),
        severity: Some(DiagnosticSeverity::ERROR),
        data: Some(serde_json::json!({
            "similar_fields": ["username", "name"]
        })),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: diagnostic.range,
        context: CodeActionContext {
            diagnostics: vec![diagnostic],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result = lsp_request_code_actions(&mut service, params, 1).await;
    let actions = result.expect("Expected actions");

    // Should have code actions for "username" and "name"
    let ca_username = find_code_action_by_title(&actions, "Change to 'username'")
        .expect("Should find 'Change to username' action");

    assert_eq!(ca_username.kind, Some(CodeActionKind::QUICKFIX));
    let edit = ca_username.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&query_uri];
    assert_eq!(edits[0].new_text, "username");
    assert_eq!(edits[0].range, range(0, 19, 0, 26));

    let ca_name = find_code_action_by_title(&actions, "Change to 'name'")
        .expect("Should find 'Change to name' action");

    let edit = ca_name.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&query_uri];
    assert_eq!(edits[0].new_text, "name");
}