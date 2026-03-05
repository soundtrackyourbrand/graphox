use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_code_actions,
    lsp_request_diagnostics, make_temp_project_with_schema, write_project_file,
};
use ahash::AHashMap;
use graphox::{config::RequiredFieldRule, config::RulesConfig};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_add_multiple_required() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("name".to_string(), RequiredFieldRule::Always(true));
    required_fields.insert("email".to_string(), RequiredFieldRule::Always(true));

    let schema = "type User { id: ID! name: String! email: String! } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id } }";
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

    let required_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert_eq!(
        required_diags.len(),
        2,
        "Should have 2 required field diagnostics (name and email)"
    );

    let first_required = required_diags[0].clone();
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: first_required.range,
        context: CodeActionContext {
            diagnostics: required_diags.iter().cloned().cloned().collect(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let add_name = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("name")
            } else {
                false
            }
        })
        .expect("Should find add name action");

    let add_email = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("email")
            } else {
                false
            }
        })
        .expect("Should find add email action");

    assert!(
        matches!(add_name, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Add required field 'name'"))
    );
    assert!(
        matches!(add_email, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Add required field 'email'"))
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_nested_required() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::Always(true));

    let schema = "type User { id: ID! posts: [Post!] } type Post { id: ID! title: String! } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { posts { title } } }";
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

    let required_diag = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .cloned()
        .expect("Should have required_field_missing diagnostic for 'id' on User");

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: required_diag.range,
        context: CodeActionContext {
            diagnostics: vec![required_diag],
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
                ca.title.contains("graphox-ignore")
            } else {
                false
            }
        })
        .expect("Should find ignore action");

    assert!(matches!(
        ignore_action,
        CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Ignore")
    ));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_add_required_to_fragment() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::Always(true));

    let schema = "type User { id: ID! name: String! } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { ...UserDetails } } fragment UserDetails on User { name }";
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

    let required_diag = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .cloned()
        .expect("Should have required_field_missing diagnostic for 'id'");

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: required_diag.range,
        context: CodeActionContext {
            diagnostics: vec![required_diag],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let add_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("Add required field 'id'")
            } else {
                false
            }
        })
        .expect("Should find add required field action");

    assert!(matches!(
        add_action,
        CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Add required field 'id'")
    ));
}
