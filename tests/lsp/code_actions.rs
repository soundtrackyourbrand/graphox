use crate::support::{
    create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, range,
};
use graphql_rust::{
    Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;
use crate::support::{make_temp_project_with_schema, write_project_file};

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_remove_unused_fragment() {
    let schema = "type Query { me: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    // tweak config for timeouts/watch behavior used in test
    config.watch_all_files = Some(false);
    config.timeouts = Some(graphql_rust::config::TimeoutConfig {
        workspace_scan_ms: 50,
        lsp_request_ms: 50,
    });

    let (mut service, _backend) = create_initialized_lsp_service(config).await;

    let frag_text = "fragment Unused on Query { me }";
    let frag_uri = write_project_file(&dir, "unused.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Also create a document with a duplicate field to exercise duplicate-field code action
    let dup_text = "query { me { id id } }";
    let dup_uri = write_project_file(&dir, "dup.graphql", dup_text);
    lsp_did_open(&mut service, dup_uri.clone(), "graphql", 1, dup_text).await;

    // Construct a diagnostic that points to the duplicated `id` field in dup.graphql
    let dup_diag = Diagnostic {
        range: Range::new(Position::new(0, 13), Position::new(0, 15)),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        ..Default::default()
    };

    // Wait for diagnostics (simulated by just checking if we get any)
    // In this test setup, diagnostics are published to the client (which we don't have a mock for here easily)
    // But we can trigger code_action directly with a diagnostic we construct.

    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, 0), Position::new(0, 31)),
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
    assert_eq!(edits[0].range, range(0, 24, 0, 24));
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_extract_to_fragment() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID name: String } type Query { me: User }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Select "{ id name }"
    let range = Range::new(Position::new(0, 11), Position::new(0, 22));
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

    let request = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let actions = result.expect("Expected actions");
    let extract_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Extract to fragment"
            } else {
                false
            }
        })
        .expect("Should find 'Extract to fragment' action");

    if let CodeActionOrCommand::CodeAction(action) = extract_action {
        let edit = action.edit.as_ref().unwrap();
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
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_remove_unused_variable() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me(id: ID): String }").unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = crate::support::create_service(config);

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetMe($id: ID, $unused: String) { me(id: $id) }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, 20), Position::new(0, 36)), // "$unused: String"
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

    let request = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let actions = result.expect("Expected actions");
    let remove_var_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Remove unused variable"
            } else {
                false
            }
        })
        .expect("Should find 'Remove unused variable' action");

    if let CodeActionOrCommand::CodeAction(action) = remove_var_action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        assert!(changes.contains_key(&query_uri));
    }
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_remove_type_only() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        ..Config::new_empty()
    };

    let (mut service, _) = crate::support::create_service(config);

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let frag_uri = Url::parse("file:///test.graphql").unwrap();
    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, 25), Position::new(0, 35)), // "@type_only"
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

    let request = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let actions = result.expect("Expected actions");
    let action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Remove @type_only directive"
            } else {
                false
            }
        })
        .expect("Should find 'Remove @type_only directive' action");

    if let CodeActionOrCommand::CodeAction(action) = action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        assert!(changes.contains_key(&frag_uri));
        let edits = &changes[&frag_uri];
        assert_eq!(edits[0].new_text, "");
    }
}

#[tokio::test]
#[ntest::timeout(100)]
async fn test_code_action_extract_to_fragment_tsx() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID name: String } type Query { me: User }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.tsx".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = crate::support::create_service(config);

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let tsx_path = base_dir.join("Component.tsx");
    let tsx_text = "const q = gql`query { me { id name } }`;";
    fs::write(&tsx_path, tsx_text).unwrap();
    let tsx_path = std::fs::canonicalize(tsx_path).unwrap();
    let tsx_uri = Url::from_file_path(&tsx_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: tsx_uri.clone(),
                            language_id: "typescriptreact".to_string(),
                            version: 1,
                            text: tsx_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Select "{ id name }" inside the template literal
    // query { me { id name } }
    // 01234567890123456789012
    //           ^         ^
    let range = Range::new(Position::new(0, 25), Position::new(0, 36));
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

    let request = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let actions = result.expect("Expected actions");
    let extract_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Extract to fragment"
            } else {
                false
            }
        })
        .expect("Should find 'Extract to fragment' action");

    if let CodeActionOrCommand::CodeAction(action) = extract_action {
        let edit = action.edit.as_ref().unwrap();
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
}
