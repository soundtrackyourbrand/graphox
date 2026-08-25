use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_remove_unused_variable_tsx() {
    let schema = "type Query { user(id: ID!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = r#"const q = gql`query($unused: String, $id: ID!) { user(id: $id) }`;"#;
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let doc = create_doc(tsx_uri.as_str(), tsx_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, tsx_text, "unused"),
        message: "Unused variable: $unused".to_string(),
        code: Some(NumberOrString::String("unused_variable".to_string())),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
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

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Remove unused variable")
        .expect("Should find 'Remove unused variable' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&tsx_uri));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_duplicate_field_tsx() {
    let schema = "type Query { me: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = r#"const q = gql`query { me { id id } }`;"#;
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let doc = create_doc(tsx_uri.as_str(), tsx_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, tsx_text, "id"),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
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

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Remove duplicate field")
        .expect("Should find 'Remove duplicate field' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&tsx_uri));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_deprecation_tsx() {
    let schema = "type User { id: ID! oldField: String @deprecated(reason: \"Use new\") } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = r#"const q = gql`query { me { oldField } }`;"#;
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let doc = create_doc(tsx_uri.as_str(), tsx_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, tsx_text, "oldField"),
        message: "Field 'oldField' is deprecated: Use new".to_string(),
        code: Some(NumberOrString::String("deprecated".to_string())),
        severity: Some(DiagnosticSeverity::WARNING),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
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

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(
        &actions,
        "Ignore deprecation with # graphox-ignore deprecated",
    )
    .expect("Should find 'Ignore deprecation with # graphox-ignore deprecated' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&tsx_uri));
    let edits = &changes[&tsx_uri];
    assert!(
        edits
            .iter()
            .any(|e| e.new_text == " # graphox-ignore deprecated"),
        "Should find an edit with the expected ignore text"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_format_tsx_selection() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = r#"const q = gql`query{ user{ id } }`"#;
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let doc = create_doc(tsx_uri.as_str(), tsx_text);
    let range = crate::support::range_for_token(&doc, tsx_text, "query{ user{ id } }");

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
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

    let ca = find_code_action_by_title(&actions, "Format GraphQL")
        .expect("Should find 'Format GraphQL' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&tsx_uri));
}
