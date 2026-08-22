use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_quickfix_only() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment Unused on Query { user }";
    let frag_uri = write_project_file(&dir, "unused.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let doc = create_doc(frag_uri.as_str(), frag_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, frag_text, "Unused"),
        message: "Unused fragment: Unused".to_string(),
        code: Some(NumberOrString::String("unused_fragment".to_string())),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: frag_uri.clone(),
        },
        range: diagnostic.range,
        context: CodeActionContext {
            diagnostics: vec![diagnostic],
            only: Some(vec![CodeActionKind::QUICKFIX]),
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    assert!(!actions.is_empty());
    for action in &actions {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            assert_eq!(
                ca.kind,
                Some(CodeActionKind::QUICKFIX),
                "Expected only QUICKFIX actions, but got: {:?}",
                ca.kind
            );
        }
    }

    let ca = find_code_action_by_title(&actions, "Remove unused fragment")
        .expect("Should find 'Remove unused fragment' action");
    assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_refactor_only() {
    let schema = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let range = crate::support::range_for_token(&doc, query_text, "{ id name }");

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range,
        context: CodeActionContext {
            diagnostics: vec![],
            only: Some(vec![CodeActionKind::REFACTOR_EXTRACT]),
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    assert!(!actions.is_empty());
    for action in &actions {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            assert!(
                ca.kind.is_some()
                    && (ca.kind == Some(CodeActionKind::REFACTOR_EXTRACT)
                        || ca.kind == Some(CodeActionKind::REFACTOR)),
                "Expected only REFACTOR actions, but got: {:?}",
                ca.kind
            );
        }
    }

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find 'Extract to fragment' action");
    assert_eq!(
        ca.kind,
        Some(CodeActionKind::REFACTOR_EXTRACT),
        "Extract to fragment should be a REFACTOR_EXTRACT action"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_source_only() {
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
            only: Some(vec![CodeActionKind::SOURCE_FIX_ALL]),
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    assert!(!actions.is_empty());
    for action in &actions {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            assert!(
                ca.kind
                    .as_ref()
                    .is_some_and(|k| k.as_str().starts_with("source")),
                "Expected only SOURCE actions, but got: {:?}",
                ca.kind
            );
        }
    }

    // Requesting the parent `source.fixAll` still returns our action, which is
    // namespaced as `source.fixAll.graphox` (a sub-kind VS Code accepts on save).
    let ca = find_code_action_by_title(&actions, "Format GraphQL")
        .expect("Should find 'Format GraphQL' action");
    assert_eq!(
        ca.kind,
        Some(CodeActionKind::new("source.fixAll.graphox")),
        "Format GraphQL should be a source.fixAll.graphox action"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_specific_source_fix_all_graphox_kind_returns_format_action() {
    // Regression: configuring the specific `source.fixAll.graphox` in
    // editor.codeActionsOnSave must still return the Format GraphQL action (it used
    // to early-bail because matching only recognised the parent `source.fixAll`).
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = "const q = gql`query{user{id}}`";
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let range = Range {
        start: Position::new(0, 14),
        end: Position::new(0, 14),
    };
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
        },
        range,
        context: CodeActionContext {
            diagnostics: vec![],
            only: Some(vec![CodeActionKind::new("source.fixAll.graphox")]),
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Format GraphQL")
        .expect("source.fixAll.graphox must return the Format GraphQL action");
    assert_eq!(ca.kind, Some(CodeActionKind::new("source.fixAll.graphox")));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_no_filter_returns_all() {
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

    assert!(!actions.is_empty(), "Should return at least one action");

    let kinds: Vec<Option<CodeActionKind>> = actions
        .iter()
        .filter_map(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                Some(ca.kind.clone())
            } else {
                None
            }
        })
        .collect();

    let unique_kinds: std::collections::HashSet<Option<CodeActionKind>> =
        kinds.iter().cloned().collect();

    assert!(
        !unique_kinds.is_empty(),
        "Should return at least one action kind, got: {:?}",
        unique_kinds
    );

    let format_action = find_code_action_by_title(&actions, "Format GraphQL");
    assert!(
        format_action.is_some(),
        "Should find Format GraphQL action when no filter is applied"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_unrelated_kind_filter_returns_no_actions() {
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
            only: Some(vec![CodeActionKind::REFACTOR_INLINE]),
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1).await;
    assert!(
        actions.is_none(),
        "Unsupported kind filters should return no actions"
    );
}
