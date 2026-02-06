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
        type Query { search: [SearchResult!]! }
        union SearchResult = User | Post
        type User { id: ID!, username: String! }
        type Post { id: ID!, title: String! }
        "#,
    )
    .unwrap();

    Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        base_dir: dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
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
}

#[tokio::test]
async fn test_hover_inside_inline_fragment() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        query {
            search {
                ... on User {
                    username
                }
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

    // Hover over 'username' inside the inline fragment
    let username_pos = text.find("username").unwrap();
    let position = {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in text.chars().enumerate() {
            if i == username_pos {
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
    };
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/hover")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(
        result.is_some(),
        "Hover should return something for 'username' in inline fragment"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field User.username"),
            "Should show field info for User.username, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }

    // Hover over 'User' type condition
    let user_pos = text.find("User").unwrap();
    let position = {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in text.chars().enumerate() {
            if i == user_pos {
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
    };
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/hover")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(
        result.is_some(),
        "Hover should return something for 'User' type condition"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("### type User"),
            "Should show type info for User, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_goto_definition_inside_inline_fragment() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        fragment UserFields on User {
            id
        }
        query {
            search {
                ... on User {
                    ...UserFields
                }
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

    // Go to definition for 'UserFields' inside the inline fragment
    let spread_pos = text.rfind("UserFields").unwrap();
    let position = {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in text.chars().enumerate() {
            if i == spread_pos {
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
    };
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

    assert!(
        result.is_some(),
        "Goto definition should return something for 'UserFields' in inline fragment"
    );
}

#[tokio::test]
async fn test_completion_inside_inline_fragment() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        query {
            search {
                ... on User {
                    
                }
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

    // Completion inside the inline fragment
    let pos1 = text.find("{").unwrap(); // first {
    let pos2 = text[pos1 + 1..].find("{").unwrap() + pos1 + 1; // second {
    let pos3 = text[pos2 + 1..].find("{").unwrap() + pos2 + 1; // third { (inside User)

    let position = {
        let mut line = 0;
        for (i, c) in text.chars().enumerate() {
            if i == pos3 + 1 {
                break;
            }
            if c == '\n' {
                line += 1;
            }
        }
        Position::new(line + 1, 20) // One line after the bracket
    };

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
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
    let result: CompletionResponse =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let items = match result {
        CompletionResponse::Array(arr) => arr,
        CompletionResponse::List(list) => list.items,
    };

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"username"),
        "Completions should include 'username', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"id"),
        "Completions should include 'id', got: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_references_inside_inline_fragment() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        fragment UserFields on User {
            id
        }
        query {
            search {
                ... on User {
                    ...UserFields
                }
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

    // Find references for 'UserFields'
    let def_pos = text.find("UserFields").unwrap();
    let position = {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in text.chars().enumerate() {
            if i == def_pos {
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
    };

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let request = Request::build("textDocument/references")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<Location>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let locations = result.expect("Expected locations");
    // Should find the definition and the spread inside the inline fragment
    assert!(
        locations.len() >= 2,
        "Should find at least 2 locations, got: {}",
        locations.len()
    );
}

#[tokio::test]
async fn test_rename_inside_inline_fragment() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        fragment UserFields on User {
            id
        }
        query {
            search {
                ... on User {
                    ...UserFields
                }
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

    // Rename 'UserFields'
    let def_pos = text.find("UserFields").unwrap();
    let position = {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in text.chars().enumerate() {
            if i == def_pos {
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
    };

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        new_name: "RenamedUserFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected workspace edit");
    let changes = edit.changes.expect("Expected changes");
    let file_changes = changes.get(&uri).expect("Expected changes for file");

    assert_eq!(
        file_changes.len(),
        2,
        "Should have 2 changes (definition and spread), got: {:?}",
        file_changes
    );
}

#[tokio::test]
async fn test_goto_definition_field_in_schema() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    initialize_service(&mut service).await;

    // Open schema first so it's in documents
    let schema_path = dir.path().join("schema.graphql");
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_uri = Url::from_file_path(&schema_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: schema_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: schema_text,
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let query_path = dir.path().join("query.graphql");
    let text = r#"
        query {
            search {
                ... on User {
                    username
                }
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

    // Go to definition for 'username'
    let username_pos = text.find("username").unwrap();
    let position = {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in text.chars().enumerate() {
            if i == username_pos {
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
    };
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

    assert!(
        result.is_some(),
        "Goto definition should return something for 'username'"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let expected_path = schema_uri.path().to_lowercase();
        let actual_path = loc.uri.path().to_lowercase();
        // Handle macOS /private/var vs /var
        let expected_path = expected_path.trim_start_matches("/private");
        let actual_path = actual_path.trim_start_matches("/private");
        assert_eq!(expected_path, actual_path);
    } else {
        panic!("Expected Scalar(Location)");
    }
}
