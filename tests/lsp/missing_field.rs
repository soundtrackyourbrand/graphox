use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_missing_field_diagnostic_with_suggestions() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! email: String! username: String! } type Query { user: User }",
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

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

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
    // Typo: "nam" instead of "name"
    let query_text = "query { user { id nam } }";
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

    // Give the LSP some time to process diagnostics
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Request diagnostics
    let params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/diagnostic")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: DocumentDiagnosticReportResult =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    // Check that we got a diagnostic about the missing field
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) => {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            let missing_field_diag = diagnostics
                .iter()
                .find(|d| {
                    d.message.contains("Field 'nam' not found")
                        && d.message.contains("Did you mean")
                })
                .expect("Should find missing field diagnostic with suggestions");

            // Verify the diagnostic has suggestions
            assert!(missing_field_diag.message.contains("'name'"));

            // Verify the diagnostic has the correct code
            assert_eq!(
                missing_field_diag.code,
                Some(NumberOrString::String("missing_field".to_string()))
            );

            // Verify data contains similar_fields
            if let Some(data) = &missing_field_diag.data {
                let similar_fields: Vec<String> =
                    serde_json::from_value::<Vec<String>>(data.get("similar_fields").unwrap().clone()).unwrap();
                assert!(!similar_fields.is_empty());
                assert!(similar_fields.contains(&"name".to_string()));
            } else {
                panic!("Diagnostic should have data with similar_fields");
            }
        }
        _ => panic!("Expected full diagnostic report"),
    }
}

#[tokio::test]
async fn test_missing_field_code_actions() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! email: String! username: String! } type Query { user: User }",
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

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

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
    // Typo: "usrname" instead of "username"
    let query_text = "query { user { id usrname } }";
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

    // Construct a diagnostic manually (in real scenario, this would come from diagnostics)
    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, 19), Position::new(0, 26)), // "usrname"
        message: "Field 'usrname' not found on type 'User'. Did you mean 'username'?".to_string(),
        code: Some(NumberOrString::String("missing_field".to_string())),
        severity: Some(DiagnosticSeverity::ERROR),
        data: Some(serde_json::json!({
            "similar_fields": ["username", "name"]
        })),
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
    
    // Should have code actions for "username" and "name"
    let username_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Change to 'username'"
            } else {
                false
            }
        })
        .expect("Should find 'Change to username' action");

    if let CodeActionOrCommand::CodeAction(action) = username_action {
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&query_uri];
        assert_eq!(edits[0].new_text, "username");
        assert_eq!(edits[0].range, Range::new(Position::new(0, 19), Position::new(0, 26)));
    }

    let name_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Change to 'name'"
            } else {
                false
            }
        })
        .expect("Should find 'Change to name' action");

    if let CodeActionOrCommand::CodeAction(action) = name_action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&query_uri];
        assert_eq!(edits[0].new_text, "name");
    }
}
