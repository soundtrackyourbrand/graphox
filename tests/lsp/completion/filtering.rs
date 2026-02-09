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

use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
async fn test_completion_selection_set_type_filtering() {
    let schema = "type Query { users: [User!]! posts: [Post!]! } type User { id: ID! username: String! } type Post { id: ID! title: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { users {  } posts {  } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let users_pos = Position::new(0, 15);
    let result = lsp_request_completion(&mut service, uri.clone(), users_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "username"));
    assert!(!items.iter().any(|i| i.label == "title"));

    let posts_pos = Position::new(0, 26);
    let result = lsp_request_completion(&mut service, uri.clone(), posts_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "title"));
    assert!(!items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
async fn test_fragment_spread_interface_filtering() {
    let schema = "type Query { nodeA: A nodeB: B } interface Node { id: ID! } type A implements Node { id: ID! name: String! } type B implements Node { id: ID! title: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment OnNode on Node { id } fragment OnA on A { name } fragment OnB on B { title } query { nodeA { ... } nodeB { ... } }";
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let dot_idx = text.find("nodeA { ...").unwrap();
    let prefix = &text[..dot_idx + "nodeA { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let users_pos = Position::new(line as u32, col as u32);

    let result = lsp_request_completion(&mut service, uri.clone(), users_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "OnA"));
    assert!(items.iter().any(|i| i.label == "OnNode"));
    assert!(!items.iter().any(|i| i.label == "OnB"));
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FIELD))
    );

    let dot_idx = text.find("nodeB { ...").unwrap();
    let prefix = &text[..dot_idx + "nodeB { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let posts_pos = Position::new(line as u32, col as u32);

    let result = lsp_request_completion(&mut service, uri.clone(), posts_pos).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "OnB"));
    assert!(items.iter().any(|i| i.label == "OnNode"));
    assert!(!items.iter().any(|i| i.label == "OnA"));
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FIELD))
    );
}

#[tokio::test]
async fn test_fragment_spread_union_filtering_extended() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { itemA: A itemB: B } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment OnItem on Item { id } fragment OnA on A { name } fragment OnB on B { title } query { itemA { ... } itemB { ... } }";
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
                            language_id: "graphql".to_string(),
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

    let file_text = text;
    let dot_idx = file_text.find("itemA { ...").unwrap();
    let prefix = &file_text[..dot_idx + "itemA { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos_a = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos_a,
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

    let dot_idx = file_text.find("itemB { ...").unwrap();
    let prefix = &file_text[..dot_idx + "itemB { ".len() + 3];
    let line = prefix.matches('\n').count();
    let col = prefix
        .lines()
        .last()
        .map(|l| l.chars().count())
        .unwrap_or(0);
    let pos_b = Position::new(line as u32, col as u32);

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: pos_b,
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
        assert!(items.iter().any(|i| i.label == "OnB"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnA"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_inside_union_type() {
    let schema = "type Query { node: Item } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { node { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "__typename"),
        "Should include __typename for union type: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "id"),
        "Should NOT include 'id' directly - union requires inline fragment: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "name"),
        "Should NOT include 'name' directly - union requires inline fragment: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "title"),
        "Should NOT include 'title' directly - union requires inline fragment: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_union_with_inline_fragments() {
    let schema = "type Query { node: Item } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { node { ... on | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "A"),
        "Should offer union member A: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "B"),
        "Should offer union member B: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "Item"),
        "Should offer union type itself: {:?}",
        labels
    );
}
