use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphql_rust::{Backend, Config};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
async fn test_embedded_fragment_spreads_interface_tsx() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { node: A item: B } interface Node { id: ID! } type A implements Node { id: ID! name: String! } type B implements Node { id: ID! title: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.tsx".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.tsx");
    let text = r#"const q = graphql(/* GraphQL */ `fragment OnNode on Node { id } fragment OnA on A { name } fragment OnB on B { title } query { node { ... } item { ... } }`);"#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let file_text = text.to_string();
    let dot_idx = file_text.find("node { ...").unwrap();
    let prefix = &file_text[..dot_idx + "node { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnA"));
        assert!(items.iter().any(|i| i.label == "OnNode"));
        assert!(!items.iter().any(|i| i.label == "OnB"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_embedded_fragment_spreads_union_tsx() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { item: A other: B } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.tsx".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("test.tsx");
    let text = r#"const q = graphql(/* GraphQL */ `fragment OnItem on Item { id } fragment OnA on A { name } fragment OnB on B { title } query { item { ... } other { ... } }`);"#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let file_text = text.to_string();
    let dot_idx = file_text.find("item { ...").unwrap();
    let prefix = &file_text[..dot_idx + "item { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnA"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnB"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_field_completion_tsx_inserts_braces_when_missing() {
    let schema = "type Query { user: User } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("const q = graphql(/* GraphQL */ `\nquery { user|\n}\n`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "user")
        .expect("Expected 'user' completion");

    let (final_text, new_pos) = crate::support::apply_completion_item(&text, position, item);

    assert!(
        final_text.contains("user {"),
        "Expected braces in TSX completion: {:?}",
        final_text
    );

    if let Some(pos) = new_pos {
        assert_eq!(pos, Position::new(2, 2));
    } else if let Some(insert_text) = &item.insert_text
        && insert_text.contains("$0")
    {
        panic!("Expected new_pos to be Some when snippet is applied");
    }
}

#[tokio::test]
async fn test_field_completion_tsx_no_braces_when_present() {
    let schema = "type Query { user: User } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("const q = graphql(/* GraphQL */ `\nquery { user| { id }\n}\n`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "user")
        .expect("Expected 'user' completion");

    if item.text_edit.is_none() {
        let insert_text = item.insert_text.as_ref().unwrap_or(&item.label);
        assert!(
            !insert_text.contains('{'),
            "Should not add braces when already present in TSX"
        );
    }
}
