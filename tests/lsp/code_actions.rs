use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_remove_unused_fragment() {
    let schema = "type Query { me: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    // tweak config for timeouts/watch behavior used in test
    config = config.with_watch_all_files(false).with_timeouts(
        graphox::config::TimeoutConfig::default()
            .with_workspace_scan_ms(50)
            .with_lsp_request_ms(50),
    );

    let (mut service, _backend) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment Unused on Query { me }";
    let frag_uri = write_project_file(&dir, "unused.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Also create a document with a duplicate field to exercise duplicate-field code action
    let dup_text = "query { me { id id } }";
    let dup_uri = write_project_file(&dir, "dup.graphql", dup_text);
    lsp_did_open(&mut service, dup_uri.clone(), "graphql", 1, dup_text).await;

    // Construct a diagnostic that points to the duplicated `id` field in dup.graphql
    let doc_dup = create_doc(dup_uri.as_str(), dup_text);
    let dup_diag = Diagnostic {
        range: crate::support::range_for_token(&doc_dup, dup_text, "id"),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        ..Default::default()
    };

    let doc_unused = create_doc(frag_uri.as_str(), frag_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc_unused, frag_text, "Unused"),
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
            diagnostics: vec![diagnostic.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");
    assert!(!actions.is_empty());

    // Find and verify 'Remove unused fragment' action
    let ca = find_code_action_by_title(&actions, "Remove unused fragment")
        .expect("Expected 'Remove unused fragment' action");
    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&frag_uri));

    // Now request code actions for the duplicate document specifically
    let params_dup = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: dup_uri.clone(),
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

    let actions_dup = lsp_request_code_actions(&mut service, params_dup, 2)
        .await
        .expect("Expected actions for dup file");
    let ca_dup = find_code_action_by_title(&actions_dup, "Remove duplicate field")
        .expect("Expected 'Remove duplicate field' action");
    let edit = ca_dup.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&dup_uri));

    let mark_ca = actions
        .iter()
        .filter_map(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                Some(ca)
            } else {
                None
            }
        })
        .find(|ca| ca.title.starts_with("Mark fragment as @type_only"))
        .expect("Should find 'Mark fragment as @type_only' action");
    let edit = mark_ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&frag_uri];
    assert_eq!(edits[0].new_text, " @type_only");
    let query_range = crate::support::range_for_token(&doc_unused, frag_text, "Query");
    let expected_pos = query_range.end;
    assert_eq!(edits[0].range, Range::new(expected_pos, expected_pos));
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_extract_to_fragment() {
    let schema = "type User { id: ID name: String } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);

    // Select "{ id name }"
    let range = crate::support::range_for_token(&doc, query_text, "{ id name }");
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
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

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find 'Extract to fragment' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&query_uri];

    assert_eq!(edits.len(), 2);
    assert!(
        edits
            .iter()
            .any(|e| e.new_text.contains("fragment NewFragment"))
    );
    assert!(edits.iter().any(|e| e.new_text.contains("...NewFragment")));
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_remove_unused_variable() {
    let schema = "type Query { me(id: ID): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query GetMe($id: ID, $unused: String) { me(id: $id) }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "unused"),
        message: "Unused variable: $unused".to_string(),
        code: Some(NumberOrString::String("unused_variable".to_string())),
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

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Remove unused variable")
        .expect("Should find 'Remove unused variable' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&query_uri));
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_remove_type_only() {
    let schema = "type Query { me: String }";
    let (_dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment F on Query @type_only { me }";
    let frag_uri = write_project_file(&_dir, "test.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let doc = create_doc(frag_uri.as_str(), frag_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, frag_text, "@type_only"),
        message: "Fragment 'F' is used but marked with @type_only".to_string(),
        code: Some(NumberOrString::String("type_only_used".to_string())),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: frag_uri.clone(),
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

    let ca = find_code_action_by_title(&actions, "Remove @type_only directive")
        .expect("Should find 'Remove @type_only directive' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&frag_uri));
    let edits = &changes[&frag_uri];
    assert_eq!(edits[0].new_text, "");
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_extract_to_fragment_tsx() {
    let schema = "type User { id: ID name: String } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.tsx");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let tsx_text = "const q = gql`query { me { id name } }`;";
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    // Select "{ id name }" inside the template literal
    let doc = create_doc(tsx_uri.as_str(), tsx_text);
    let range = crate::support::range_for_token(&doc, tsx_text, "{ id name }");
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

    let ca = find_code_action_by_title(&actions, "Extract to fragment")
        .expect("Should find 'Extract to fragment' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&tsx_uri];

    assert_eq!(edits.len(), 2);
    assert!(
        edits
            .iter()
            .any(|e| e.new_text.contains("fragment NewFragment"))
    );
    assert!(edits.iter().any(|e| e.new_text.contains("...NewFragment")));
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_type_only_removal() {
    let schema = "type Query { me: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment F on Query @type_only { me }";
    let frag_uri = write_project_file(&dir, "test.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let doc = create_doc(frag_uri.as_str(), frag_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, frag_text, "@type_only"),
        message: "Fragment 'F' is used but marked with @type_only".to_string(),
        code: Some(NumberOrString::String("type_only_used".to_string())),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: frag_uri.clone(),
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

    let ca = find_code_action_by_title(&actions, "Remove @type_only directive")
        .expect("Should find 'Remove @type_only directive' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&frag_uri));
    let edits = &changes[&frag_uri];
    assert_eq!(edits[0].new_text, "");
}

#[tokio::test]
#[ntest::timeout(100)]
#[ignore] // Not implemented feature
async fn test_code_action_variable_definition() {
    let schema = "type Query { user(id: ID!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query GetUser { user(id: $id) }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "$id"),
        message: "Undefined variable: $id".to_string(),
        code: Some(NumberOrString::String("undefined_variable".to_string())),
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

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ca = find_code_action_by_title(&actions, "Add variable definition")
        .expect("Should find 'Add variable definition' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&query_uri));
}
