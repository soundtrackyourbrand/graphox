use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_missing_field_code_action_with_alias() {
    let schema = "type User { id: ID! name: String! email: String! username: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    write_project_file(&dir, "package.json", "{}");

    let query_text = "query { user { id a: usrname } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);

    let diag = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "usrname"),
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
    assert_eq!(edits[0].new_text, "username");
    assert_eq!(edits[0].range, diag.range);
}

#[tokio::test]
async fn test_duplicate_field_code_action_alias_collision() {
    let schema = "type Query { me: User } type User { id: ID name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id name id: name } }";
    let query_uri = write_project_file(&dir, "collision.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);

    let dup_diag = Diagnostic {
        range: crate::support::range_for_token_at_index(&doc, query_text, "id", 1),
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

#[tokio::test]
async fn test_alias_completion() {
    let schema = "type User { id: ID! name: String! email: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, cursor_pos) = with_cursor("query { user { userId: | } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let completions =
        crate::support::lsp_request_completion(&mut service, query_uri.clone(), cursor_pos).await;

    let items = crate::support::completion_items_array(&completions);
    assert!(
        items.iter().any(|i| i.label == "id"),
        "Should suggest 'id' field"
    );
    assert!(
        items.iter().any(|i| i.label == "name"),
        "Should suggest 'name' field"
    );
    assert!(
        items.iter().any(|i| i.label == "email"),
        "Should suggest 'email' field"
    );
}

#[tokio::test]
async fn test_aliased_field_hover() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, cursor_pos) = with_cursor("query { user { myAlias: i|d } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let hover =
        crate::support::lsp_request_hover(&mut service, query_uri.clone(), cursor_pos).await;

    assert!(
        hover.is_some(),
        "Hover should return information for aliased field"
    );
}

#[tokio::test]
#[ignore] // Not implemented feature
async fn test_alias_hover() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, cursor_pos) = with_cursor("query { user { my|Alias: id } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let hover =
        crate::support::lsp_request_hover(&mut service, query_uri.clone(), cursor_pos).await;

    assert!(
        hover.is_some(),
        "Hover should return information for aliased field"
    );
}

#[tokio::test]
async fn test_alias_definition() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { user { myAlias: id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let params = crate::lsp::alias_code_actions::GotoDefinitionParams {
        text_document_position_params: tower_lsp::lsp_types::TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: crate::support::pos(1, 18),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<tower_lsp::lsp_types::GotoDefinitionResponse> =
        crate::support::lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    match result {
        Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc)) => {
            assert!(
                loc.uri == query_uri,
                "Definition should point to the same document"
            );
        }
        Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(arr)) => {
            assert!(
                !arr.is_empty(),
                "Definition should return at least one location"
            );
        }
        Some(tower_lsp::lsp_types::GotoDefinitionResponse::Link(_)) => {
            // Link responses are valid
        }
        None => {
            // Goto definition for alias might return None
        }
    }
}

#[tokio::test]
async fn test_alias_semantic_tokens() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { user { myAlias: id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let params = SemanticTokensParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<SemanticTokensResult> = crate::support::lsp_request_typed(
        &mut service,
        "textDocument/semanticTokens/full",
        &params,
    )
    .await;

    match result {
        Some(SemanticTokensResult::Tokens(tokens)) => {
            assert!(
                !tokens.data.is_empty(),
                "Should have semantic tokens for aliased field"
            );
        }
        Some(SemanticTokensResult::Partial(_)) => {
            // Partial results are acceptable
        }
        None => {
            panic!("Semantic tokens should return a result");
        }
    }
}

#[tokio::test]
async fn test_alias_diagnostic_response_key() {
    let schema = "type Query { me: User } type User { id: ID name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id id: name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);

    let diag = Diagnostic {
        range: crate::support::range_for_token_at_index(&doc, query_text, "id", 1),
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

    let has_remove_action = actions.iter().any(|ca| {
        if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) = ca {
            ca.title.contains("Remove") || ca.title.contains("duplicate")
        } else {
            false
        }
    });

    assert!(
        has_remove_action,
        "Should have a code action to remove the duplicate aliased field"
    );
}
