use graphql_rust::{
    Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_smart_extract_fragment() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    // Selection set of 'me' is on line 1, column 11 to 27
    let query_text = "query {\n  me {\n    id\n    name\n  }\n}";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        base_dir: base_dir.clone(),
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
        .unwrap();

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

    // Request code actions for the selection set of 'me' ({ id name })
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: Range::new(Position::new(1, 5), Position::new(4, 3)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = service
        .call(
            Request::build("textDocument/codeAction")
                .params(serde_json::to_value(params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let actions = result.unwrap();

    let extract_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Extract to fragment"
            } else {
                false
            }
        })
        .expect("Should find extract action");

    if let CodeActionOrCommand::CodeAction(ca) = extract_action {
        let edit = ca.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let file_changes = changes.get(&query_uri).unwrap();

        // One edit should be the fragment definition at the end
        let fragment_def_edit = file_changes
            .iter()
            .find(|e| e.new_text.contains("fragment"))
            .unwrap();
        assert!(
            fragment_def_edit.new_text.contains("on User"),
            "Fragment should be on type User, got: {}",
            fragment_def_edit.new_text
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_smart_extract_field() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query {\n  me {\n    id\n  }\n}";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        base_dir: base_dir.clone(),
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
        .unwrap();

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

    // Request code actions for the field 'me'
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: Range::new(Position::new(1, 2), Position::new(1, 4)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = service
        .call(
            Request::build("textDocument/codeAction")
                .params(serde_json::to_value(params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let actions = result.unwrap();

    let extract_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Extract to fragment"
            } else {
                false
            }
        })
        .expect("Should find extract action");

    if let CodeActionOrCommand::CodeAction(ca) = extract_action {
        let edit = ca.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let file_changes = changes.get(&query_uri).unwrap();

        let fragment_def_edit = file_changes
            .iter()
            .find(|e| e.new_text.contains("fragment"))
            .unwrap();
        assert!(
            fragment_def_edit.new_text.contains("on Query"),
            "Fragment for field 'me' should be on type Query, got: {}",
            fragment_def_edit.new_text
        );
    }
}
