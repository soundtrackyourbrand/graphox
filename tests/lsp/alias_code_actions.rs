use crate::support::{
    create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, range, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_missing_field_code_action_with_alias() {
    let schema = "type User { id: ID! name: String! email: String! username: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // write additional files
    write_project_file(&dir, "package.json", "{}");

    // Alias 'a' targets a misspelled field 'usrname'
    let query_text = "query { user { id a: usrname } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Construct a diagnostic pointing at the inner (misspelled) name `usrname`
    let start = query_text.find("usrname").expect("token exists");
    let end = start + "usrname".len();

    let diag = Diagnostic {
        range: range(0, start as u32, 0, end as u32),
        message: "Field 'usrname' not found on type 'User'. Did you mean 'username'?".to_string(),
        code: Some(NumberOrString::String("missing_field".to_string())),
        severity: Some(DiagnosticSeverity::ERROR),
        data: Some(serde_json::json!({ "similar_fields": ["username", "name"] })),
        ..Default::default()
    };

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

    let ca = find_code_action_by_title(&actions, "Change to 'username'")
        .expect("Should find 'Change to username' action");

    assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&query_uri];
    // Ensure the edit replaces the misspelled name (not the alias)
    assert_eq!(edits[0].new_text, "username");
    assert_eq!(edits[0].range, diag.range);
}

#[tokio::test]
async fn test_duplicate_field_code_action_alias_collision() {
    let schema = "type Query { me: User } type User { id: ID name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Unaliased `id` and an aliased `id: name` collide (response key 'id')
    let query_text = "query { me { id name id: name } }";
    let query_uri = write_project_file(&dir, "collision.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find the start of the aliased occurrence `id: name` to point diagnostic there
    let token = "id: name";
    let start = query_text.find(token).expect("token exists");
    let alias_name_start = start; // 'id' alias starts here
    let alias_name_end = alias_name_start + 2; // 'id' length

    let dup_diag = Diagnostic {
        range: range(0, alias_name_start as u32, 0, alias_name_end as u32),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        severity: Some(DiagnosticSeverity::ERROR),
        data: Some(serde_json::json!({ "response_key": "id", "args": "", "selection": "" })),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: dup_diag.range,
        context: CodeActionContext {
            diagnostics: vec![dup_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 2)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Remove duplicate field")
        .expect("Expected 'Remove duplicate field' action for alias collision");

    assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
}