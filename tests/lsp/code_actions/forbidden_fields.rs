use crate::support::{
    apply_text_edit, create_initialized_lsp_service, lsp_did_open, lsp_request_code_actions,
    lsp_request_diagnostics, make_temp_project_with_schema, write_project_file,
};
use ahash::AHashMap;
use graphox::{config::ForbiddenFieldRule, config::RulesConfig};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_remove_forbidden_field() {
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let schema = "type User { id: ID! name: String! password: String! } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query GetUser {\n  me {\n    id\n    password\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let diag_result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let diagnostics = if let DocumentDiagnosticReportResult::Report(
        DocumentDiagnosticReport::Full(full_report),
    ) = diag_result
    {
        full_report.full_document_diagnostic_report.items
    } else {
        panic!("Expected full diagnostic report");
    };

    let forbidden_diags: Vec<Diagnostic> = diagnostics
        .into_iter()
        .filter(|d| {
            d.code
                == Some(NumberOrString::String(
                    "forbidden_field_selected".to_string(),
                ))
        })
        .collect();

    assert_eq!(
        forbidden_diags.len(),
        1,
        "Should have 1 forbidden field diagnostic"
    );

    let diag = forbidden_diags[0].clone();
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: diag.range,
        context: CodeActionContext {
            diagnostics: vec![diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let remove_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Remove forbidden field 'password'"
            } else {
                false
            }
        })
        .expect("Should find remove forbidden field action");

    if let CodeActionOrCommand::CodeAction(ca) = remove_action {
        let edits = ca
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .get(&query_uri)
            .unwrap();
        let new_text = apply_text_edit(query_text, &edits[0]);
        assert!(
            !new_text.contains("password"),
            "Field 'password' should be removed"
        );
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_forbidden_field() {
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let schema = "type User { id: ID! name: String! password: String! } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query GetUser {\n  me {\n    id\n    password\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let diag_result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let diagnostics = if let DocumentDiagnosticReportResult::Report(
        DocumentDiagnosticReport::Full(full_report),
    ) = diag_result
    {
        full_report.full_document_diagnostic_report.items
    } else {
        panic!("Expected full diagnostic report");
    };

    let forbidden_diags: Vec<Diagnostic> = diagnostics
        .into_iter()
        .filter(|d| {
            d.code
                == Some(NumberOrString::String(
                    "forbidden_field_selected".to_string(),
                ))
        })
        .collect();

    assert_eq!(forbidden_diags.len(), 1);

    let diag = forbidden_diags[0].clone();
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: diag.range,
        context: CodeActionContext {
            diagnostics: vec![diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ignore_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Ignore forbidden field with # graphox-ignore"
            } else {
                false
            }
        })
        .expect("Should find ignore action");

    if let CodeActionOrCommand::CodeAction(ca) = ignore_action {
        let edits = ca
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .get(&query_uri)
            .unwrap();
        let new_text = apply_text_edit(query_text, &edits[0]);
        assert!(
            new_text.contains("password # graphox-ignore"),
            "Should add ignore comment"
        );
    }
}
