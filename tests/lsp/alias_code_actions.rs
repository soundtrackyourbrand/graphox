use crate::support::{
    make_temp_project_with_schema, create_initialized_lsp_service, write_project_file, lsp_did_open,
};
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
        range: Range::new(Position::new(0, start as u32), Position::new(0, end as u32)),
        message: "Field 'usrname' not found on type 'User'. Did you mean 'username'?".to_string(),
        code: Some(NumberOrString::String("missing_field".to_string())),
        severity: Some(DiagnosticSeverity::ERROR),
        data: Some(serde_json::json!({ "similar_fields": ["username", "name"] })),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: query_uri.clone() },
        range: diag.range,
        context: CodeActionContext {
            diagnostics: vec![diag.clone()],
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
    let result: Option<CodeActionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let actions = result.expect("Expected actions");

    // Should have a quickfix to change to 'username'
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
        // Ensure the edit replaces the misspelled name (not the alias)
        assert_eq!(edits[0].new_text, "username");
        assert_eq!(edits[0].range, diag.range);
    }
}

#[tokio::test]
async fn test_duplicate_field_code_action_alias_collision() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    fs::write(
        base_dir.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
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

    let query_path = base_dir.join("collision.graphql");
    // Unaliased `id` and an aliased `id: name` collide (response key 'id')
    let query_text = "query { me { id name id: name } }";
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

    // Find the start of the aliased occurrence `id: name` to point diagnostic there
    let token = "id: name";
    let start = query_text.find(token).expect("token exists");
    let alias_name_start = start; // 'id' alias starts here
    let alias_name_end = alias_name_start + 2; // 'id' length

    let dup_diag = Diagnostic {
        range: Range::new(
            Position::new(0, alias_name_start as u32),
            Position::new(0, alias_name_end as u32),
        ),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        severity: Some(DiagnosticSeverity::ERROR),
        data: Some(serde_json::json!({ "response_key": "id", "args": "", "selection": "" })),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: query_uri.clone() },
        range: dup_diag.range,
        context: CodeActionContext {
            diagnostics: vec![dup_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/codeAction")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let actions = result.expect("Expected actions");

    // Should include a 'Remove duplicate field' quickfix
    let remove_action = actions
        .iter()
        .find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove duplicate field",
            _ => false,
        })
        .expect("Expected 'Remove duplicate field' action for alias collision");

    if let CodeActionOrCommand::CodeAction(ca) = remove_action {
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
    }
}
