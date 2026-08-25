use crate::support::{
    create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use graphox::{config::RequiredFieldRule, config::RulesConfig};
use tower_lsp_server::ls_types::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_smart_extract_fragment() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "query.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

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
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

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
    use ahash::AHashMap;

    // Create config with required field rule
    let mut required_fields = AHashMap::default();
    required_fields.insert("requestId".to_string(), RequiredFieldRule::new_always(true));

    let schema = "type User { id: ID! name: String } type Query { me: User requestId: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "query.graphql");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

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
    assert_eq!(
        required_field_diagnostic.range,
        Range::new(Position::new(1, 2), Position::new(1, 4))
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

    let ignore_ca = find_code_action_by_title(
        &actions,
        "Ignore required field with # graphox-ignore required_fields",
    )
    .expect("Should find ignore required field action");
    let ignore_edit = ignore_ca.edit.as_ref().unwrap();
    let ignore_changes = ignore_edit.changes.as_ref().unwrap();
    let ignore_file_changes = ignore_changes.get(&query_uri).unwrap();
    assert_eq!(ignore_file_changes.len(), 1);
    assert_eq!(
        ignore_file_changes[0].new_text,
        " # graphox-ignore required_fields"
    );
    assert_eq!(
        ignore_file_changes[0].range,
        Range::new(Position::new(1, 6), Position::new(1, 6))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_required_field_nested_code_action_targets_nested_selection() {
    use ahash::AHashMap;

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "permissions".to_string(),
        RequiredFieldRule::new_always(true),
    );

    let schema = "type Query { radioPlaylist(id: ID!, kind: RadioPlaylistKind): RadioPlaylist } enum RadioPlaylistKind { A } interface Displayable { id: ID } type RadioPlaylist { id: ID permissions: String playlist: Playlist } type Playlist implements Displayable { id: ID permissions: String name: String composerType: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.{tsx,graphql}");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let fragment_text = r#"fragment Displayable on Displayable { id }"#;
    let fragment_uri = write_project_file(&dir, "fragments.graphql", fragment_text);
    lsp_did_open(&mut service, fragment_uri, "graphql", 1, fragment_text).await;

    let tsx_text = r#"export const SourceRadioDoc = graphql(/* GraphQL */ `
  query SourceRadio($id: ID!, $kind: RadioPlaylistKind) {
    # eslint-disable-next-line @graphql-eslint/require-id-when-available
    radioPlaylist(id: $id, kind: $kind) {
      id,
      playlist {
        permissions,
        name,
        composerType,
        id,
        ...Displayable,
      }
    }
  }
`)
"#;
    let tsx_uri = write_project_file(&dir, "query.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let diag_result = crate::support::lsp_request_diagnostics(&mut service, tsx_uri.clone()).await;
    let diagnostics = if let DocumentDiagnosticReportResult::Report(
        DocumentDiagnosticReport::Full(full_report),
    ) = diag_result
    {
        full_report.full_document_diagnostic_report.items
    } else {
        panic!("Expected full diagnostic report");
    };

    let required_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String("required_field_missing".to_string()))
                && d.message.contains("'radioPlaylist'")
        })
        .cloned()
        .collect();

    assert_eq!(
        required_diagnostics.len(),
        1,
        "Should emit a single required_field_missing diagnostic for radioPlaylist, got: {:?}",
        required_diagnostics
    );

    let required_field_diagnostic = required_diagnostics[0].clone();
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
        },
        range: required_field_diagnostic.range,
        context: CodeActionContext {
            diagnostics: vec![required_field_diagnostic],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 3)
        .await
        .expect("Expected actions");

    let add_ca = find_code_action_by_title(&actions, "Add required field 'permissions'")
        .expect("Should find add required field action");
    let add_changes = add_ca
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .and_then(|changes| changes.get(&tsx_uri))
        .expect("Expected text edit for query.tsx");

    assert_eq!(add_changes.len(), 1);
    assert_eq!(
        add_changes[0].range.start.line, 3,
        "Required field should be inserted inside radioPlaylist selection set"
    );
    assert!(
        add_changes[0].new_text.contains("permissions"),
        "Expected permissions insertion edit, got: {}",
        add_changes[0].new_text
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_required_field_code_actions_dedup_duplicate_diagnostics() {
    use ahash::AHashMap;

    let mut required_fields = AHashMap::default();
    required_fields.insert("requestId".to_string(), RequiredFieldRule::new_always(true));

    let schema = "type User { id: ID! name: String } type Query { me: User requestId: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "query.graphql");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query {\n  me {\n    id\n    name\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

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
        .expect("Should have required_field_missing diagnostic")
        .clone();

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: required_field_diagnostic.range,
        context: CodeActionContext {
            diagnostics: vec![
                required_field_diagnostic.clone(),
                required_field_diagnostic.clone(),
            ],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 4)
        .await
        .expect("Expected actions");

    let required_actions = actions
        .iter()
        .filter_map(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                Some(ca.title.as_str())
            } else {
                None
            }
        })
        .filter(|title| {
            *title == "Add required field 'requestId'"
                || *title == "Ignore required field with # graphox-ignore required_fields"
        })
        .count();

    assert_eq!(
        required_actions, 2,
        "Should only return one add + one ignore action for duplicate required diagnostics"
    );
}
