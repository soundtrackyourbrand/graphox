use graphox::{
    Backend, Config,
    config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::*;
use tower_service::Service;

use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_fragment_spread() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("fragment MyFrag on User { id } query { users { ...| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "MyFrag"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_types_in_fragment() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("fragment MyFrag on | { id }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "User"),
        "Expected User in completions: {:?}",
        labels
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_fragment_spread_acceptance() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("fragment MyFrag on User { id }\nquery { users { ...| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "MyFrag")
        .expect("MyFrag completion not found");

    let (final_text, _pos) = crate::support::apply_completion_item(&text, position, item);

    assert_eq!(
        final_text,
        "fragment MyFrag on User { id }\nquery { users { ...MyFrag } }"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_inline_fragment_completion_tsx_inserts_braces_when_missing() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    ... on |\n  }\n}\n`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    let (final_text, new_pos) = crate::support::apply_completion_item(&text, position, item);

    assert!(final_text.contains("... on User {"));

    if let Some(pos) = new_pos {
        assert_eq!(pos, Position::new(4, 6));
    } else if let Some(insert_text) = &item.insert_text
        && insert_text.contains("$0")
    {
        panic!("Expected new_pos to be Some when snippet is applied");
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_inline_fragment_completion_tsx_no_braces_when_present() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    ... on | { id }\n  }\n}\n`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    if item.text_edit.is_none() {
        let insert_text = item.insert_text.as_ref().unwrap_or(&item.label);
        assert!(!insert_text.contains('{'));
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_fragment_spread_type_filtering() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! posts: [Post!]! } type User { id: ID! username: String! } type Post { id: ID! title: String! }",
    )
    .unwrap();

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("test.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _) =
        LspService::new(|client| graphox::GraphoxLanguageServer::new(Backend::new(client, config)));
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment OnUser on User { id } fragment OnPost on Post { id } query { users { ... } posts { ... } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = graphox::utils::path_to_uri(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    let file_text = text;
    let dot_idx = file_text.find("users { ...").unwrap();
    let prefix = &file_text[..dot_idx + "users { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let users_pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: users_pos,
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
        assert!(items.iter().any(|i| i.label == "OnUser"));
        assert!(!items.iter().any(|i| i.label == "OnPost"));
        assert!(
            !items
                .iter()
                .any(|i| i.kind == Some(CompletionItemKind::FIELD))
        );
    } else {
        panic!("Expected array of completions");
    }

    let dot_idx = file_text.find("posts { ...").unwrap();
    let prefix = &file_text[..dot_idx + "posts { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let posts_pos = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: posts_pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let request = Request::build("textDocument/completion")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnPost"));
        assert!(!items.iter().any(|i| i.label == "OnUser"));
        assert!(
            !items
                .iter()
                .any(|i| i.kind == Some(CompletionItemKind::FIELD))
        );
    } else {
        panic!("Expected array of completions");
    }
}
