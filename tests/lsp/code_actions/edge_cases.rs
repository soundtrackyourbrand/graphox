use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_remove_variable_multiple() {
    let schema = "type Query { user(id: ID!): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query($unused1: String, $unused2: Int, $id: ID!) { user(id: $id) }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "unused1"),
        message: "Unused variable: $unused1".to_string(),
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
#[ntest::timeout(3000)]
async fn test_variable_with_default() {
    let schema = "type Query { user(status: String): String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query($status: String = \"active\", $unused: Int) { user(status: $status) }";
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
#[ntest::timeout(3000)]
async fn test_remove_first_duplicate() {
    let schema = "type Query { me: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id id name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "id"),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
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

    let ca = find_code_action_by_title(&actions, "Remove duplicate field")
        .expect("Should find 'Remove duplicate field' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&query_uri));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_duplicate_with_alias() {
    let schema = "type Query { me: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id alias: id name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "id"),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
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

    let ca = find_code_action_by_title(&actions, "Remove duplicate field")
        .expect("Should find 'Remove duplicate field' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    assert!(changes.contains_key(&query_uri));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_type_only_fragment_used() {
    let schema = "type Query { me: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment F on Query @type_only { me }";
    let frag_uri = write_project_file(&dir, "fragment.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let query_text = "query { ...F }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

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
