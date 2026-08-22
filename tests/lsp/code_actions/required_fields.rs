use crate::support::{
    apply_text_edit, create_initialized_lsp_service, lsp_did_open, lsp_request_code_actions,
    lsp_request_diagnostics, make_temp_project_with_schema, write_project_file,
};
use ahash::AHashMap;
use graphox::{config::RequiredFieldRule, config::RulesConfig};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_add_multiple_required() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("name".to_string(), RequiredFieldRule::new_always(true));
    required_fields.insert("email".to_string(), RequiredFieldRule::new_always(true));

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

    let required_diags: Vec<Diagnostic> = diagnostics
        .into_iter()
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
            diagnostics: required_diags.clone(),
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
                ca.title == "Add required field 'name'"
            } else {
                false
            }
        })
        .expect("Should find add name action");

    let add_email = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Add required field 'email'"
            } else {
                false
            }
        })
        .expect("Should find add email action");

    assert!(
        matches!(add_name, CodeActionOrCommand::CodeAction(ca) if ca.title == "Add required field 'name'")
    );
    assert!(
        matches!(add_email, CodeActionOrCommand::CodeAction(ca) if ca.title == "Add required field 'email'")
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_nested_required() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

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
                ca.title == "Ignore required field with # graphox-ignore"
            } else {
                false
            }
        })
        .expect("Should find ignore action");

    assert!(matches!(
        ignore_action,
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Ignore required field with # graphox-ignore"
    ));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_add_required_to_fragment() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

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
                ca.title == "Add required field 'id'"
            } else {
                false
            }
        })
        .expect("Should find add required field action");

    assert!(matches!(
        add_action,
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Add required field 'id'"
    ));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_add_required_field_targets_nested_response_key_selection() {
    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let schema = r#"
        type Query { viewer: Viewer }
        type Viewer { posts: PostConnection! }
        type PostConnection { edges: [PostEdge!]! }
        type PostEdge { node: Post }
        type Post { id: ID! title: String! }
    "#;
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = r#"query GetViewer {
  viewer {
    posts {
      edges {
        node {
          title
        }
      }
    }
  }
}"#;
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
        .find(|d| {
            d.code == Some(NumberOrString::String("required_field_missing".to_string()))
                && d.range.start.line == 4
                && d.range.start.character == 8
        })
        .cloned()
        .expect("Should have required_field_missing diagnostic for nested 'node' at line 4");

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
        .find_map(|action| {
            let CodeActionOrCommand::CodeAction(code_action) = action else {
                return None;
            };
            (code_action.title == "Add required field 'id'").then_some(code_action)
        })
        .expect("Should find add required field action for nested response key");

    let edits = add_action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .and_then(|changes| changes.get(&query_uri))
        .expect("Code action should provide edits for the query document");

    let mut updated = query_text.to_string();
    for edit in edits {
        updated = apply_text_edit(&updated, edit);
    }

    // Semantic assertions: check that 'id' is added within the 'node' block and not at root
    let node_block = updated
        .split("node {")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .expect("Should find node selection set block in updated query");

    assert!(
        node_block.contains("id"),
        "Required-field fix should insert 'id' inside the nested node selection set. updated=\n{}",
        updated
    );
    assert!(
        node_block.contains("title"),
        "Existing field 'title' must be preserved. updated=\n{}",
        updated
    );

    let op_body = updated
        .split("query GetViewer {")
        .nth(1)
        .expect("Should find operation start");

    let mut depth = 1;
    let mut found_id_at_root = false;
    let mut chars = op_body.chars().peekable();
    let mut current_pos = 0;

    while let Some(c) = chars.next() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                break;
            }
        } else if depth == 1 && c == 'i' && chars.peek() == Some(&'d') {
            // Check if it's the standalone token "id"
            let prev_char = if current_pos > 0 {
                op_body.chars().nth(current_pos - 1)
            } else {
                None
            };
            chars.next(); // consume 'd'
            let next_char = chars.peek();

            let is_id_token = prev_char.is_none_or(|p| !p.is_alphanumeric())
                && next_char.is_none_or(|n| !n.is_alphanumeric());

            if is_id_token {
                found_id_at_root = true;
                break;
            }
            current_pos += 1; // account for consumed 'd'
        }
        current_pos += 1;
    }

    assert!(
        !found_id_at_root,
        "Required-field fix must not insert the nested field at the operation root. updated=\n{}",
        updated
    );
}
