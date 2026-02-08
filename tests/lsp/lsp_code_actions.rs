use crate::support::{
    create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use graphql_rust::{config::RequiredFieldRule, config::RulesConfig};
use tower_lsp::lsp_types::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_smart_extract_fragment() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "query.graphql");
    config.enable_schema_cache = Some(true);
    config.lsp_automatic_codegen = Some(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query {\n  me {\n    id\n    name\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Request code actions for the selection set of 'me' ({ id name })
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: Range::new(Position::new(1, 5), Position::new(4, 3)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find extract action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let file_changes = changes.get(&query_uri).unwrap();

    // One edit should be the fragment definition at the end
    let fragment_def_edit = file_changes
        .iter()
        .find(|e| e.new_text.contains("fragment"))
        .unwrap();
    assert!(
        fragment_def_edit.new_text.contains("on User"),
        "Fragment should be on type User, got: {}",
        fragment_def_edit.new_text
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_smart_extract_field() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "query.graphql");
    config.enable_schema_cache = Some(true);
    config.lsp_automatic_codegen = Some(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query {\n  me {\n    id\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Request code actions for the field 'me'
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: Range::new(Position::new(1, 2), Position::new(1, 4)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find extract action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let file_changes = changes.get(&query_uri).unwrap();

    let fragment_def_edit = file_changes
        .iter()
        .find(|e| e.new_text.contains("fragment"))
        .unwrap();
    assert!(
        fragment_def_edit.new_text.contains("on Query"),
        "Fragment for field 'me' should be on type Query, got: {}",
        fragment_def_edit.new_text
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_required_field_code_action() {
    use fnv::FnvHashMap;

    // Create config with required field rule
    let mut required_fields = FnvHashMap::default();
    required_fields.insert("requestId".to_string(), RequiredFieldRule::Always(true));

    let schema = "type User { id: ID! name: String } type Query { me: User requestId: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "query.graphql");
    config.rules = Some(RulesConfig {
        required_fields: Some(required_fields),
        ..RulesConfig::default()
    });

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query {\n  me {\n    id\n    name\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Get diagnostics first to verify the required field error exists
    let diag_result =
        crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;

    let diagnostics = if let DocumentDiagnosticReportResult::Report(
        DocumentDiagnosticReport::Full(full_report),
    ) = diag_result
    {
        full_report.full_document_diagnostic_report.items
    } else {
        panic!("Expected full diagnostic report");
    };

    let required_field_diagnostic = diagnostics
        .iter()
        .find(|d| {
            if let Some(NumberOrString::String(code)) = &d.code {
                code == "required_field_missing"
            } else {
                false
            }
        })
        .expect("Should have required_field_missing diagnostic");

    assert!(
        required_field_diagnostic.message.contains("requestId"),
        "Diagnostic should mention 'requestId'"
    );

    // Request code actions for the diagnostic
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: required_field_diagnostic.range,
        context: CodeActionContext {
            diagnostics: vec![required_field_diagnostic.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 2)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Add required field 'requestId'")
        .expect("Should find add required field action");

    assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(ca.is_preferred, Some(true));

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let file_changes = changes.get(&query_uri).unwrap();

    assert_eq!(file_changes.len(), 1);
    let text_edit = &file_changes[0];

    // The edit should add 'requestId' to the selection set
    assert!(
        text_edit.new_text.contains("requestId"),
        "Edit should add 'requestId', got: {}",
        text_edit.new_text
    );
}
