use crate::support::{
    lsp_did_open, lsp_request_diagnostics, range, make_temp_project_with_schema,
    create_initialized_lsp_service, write_project_file,
};
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

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

    // Check that we got a diagnostic about the missing field
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) => {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            let missing_field_diag = diagnostics
                .iter()
                .find(|d| {
                    d.message.contains("Field 'nam' not found")
                        && d.message.contains("Did you mean")
                })
                .expect("Should find missing field diagnostic with suggestions");

            // Verify the diagnostic has suggestions
            assert!(missing_field_diag.message.contains("'name'"));

            // Verify the diagnostic has the correct code
            assert_eq!(
                missing_field_diag.code,
                Some(NumberOrString::String("missing_field".to_string()))
            );

            // Verify data contains similar_fields
            if let Some(data) = &missing_field_diag.data {
                let similar_fields: Vec<String> = serde_json::from_value::<Vec<String>>(
                    data.get("similar_fields").unwrap().clone(),
                )
                .unwrap();
                assert!(!similar_fields.is_empty());
                assert!(similar_fields.contains(&"name".to_string()));
            } else {
                panic!("Diagnostic should have data with similar_fields");
            }
        }
        _ => panic!("Expected full diagnostic report"),
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

    let request = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let actions = result.expect("Expected actions");

    // Should have code actions for "username" and "name"
    let username_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Change to 'username'"
            } else {
                false
            }
        })
        .expect("Should find 'Change to username' action");

    if let CodeActionOrCommand::CodeAction(action) = username_action {
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&query_uri];
        assert_eq!(edits[0].new_text, "username");
        assert_eq!(edits[0].range, range(0, 19, 0, 26));
    }

    let name_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Change to 'name'"
            } else {
                false
            }
        })
        .expect("Should find 'Change to name' action");

    if let CodeActionOrCommand::CodeAction(action) = name_action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&query_uri];
        assert_eq!(edits[0].new_text, "name");
    }
}
