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

fn create_test_config(dir: &std::path::Path) -> Config {
    let schema_path = dir.join("schema.graphql");
    fs::write(
        &schema_path,
        r#"
        type Query {
            me: User!
        }
        type User {
            id: ID!
        }
        extend type User {
            username: String!
        }
        "#,
    )
    .unwrap();

    Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Multiple(vec![
                "**/*.graphql".to_string(),
                "**/*.tsx".to_string(),
            ]),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        base_dir: dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        ..Config::new_empty()
    }
}

async fn initialize_service(service: &mut LspService<Backend>) {
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Wait for workspace scan to complete (it's spawned in background)
    // In a real scenario we'd use progress notifications, but for tests
    // we just give it a moment or ensure it's loaded.
    // NOTE: Even if we don't sleep, schemas should now be loaded synchronously in Backend::new
}

#[tokio::test]
async fn test_goto_definition_fields_and_extensions() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    // NOTE: We are NOT opening the schema.graphql file here.
    // It should be loaded via workspace scan or as a project schema.

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        query {
            me {
                id
                username
            }
        }
    "#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
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

    // 1. Go to definition for 'id' (regular field)
    let id_pos = text.find("id").unwrap();
    let position = get_position(text, id_pos);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/definition")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(result.is_some(), "Should find definition for 'id'");

    // 2. Go to definition for 'username' (extended field)
    let username_pos = text.find("username").unwrap();
    let position = get_position(text, username_pos);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/definition")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(
        result.is_some(),
        "Should find definition for 'username' in extension"
    );
}

fn get_position(text: &str, pos: usize) -> Position {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i == pos {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position::new(line, col)
}
