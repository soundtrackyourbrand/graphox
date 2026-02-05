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
async fn test_completion_fields() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "query { users {  } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions at "users { | }"
    let position = Position::new(0, 16);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "id"));
        assert!(items.iter().any(|i| i.label == "username"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_variables() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user(id: ID!): User } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "query GetUser($userId: ID!) { user(id: $) }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions at "user(id: $|)"
    let position = Position::new(0, 40);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
        assert!(
            items.iter().any(|i| i.label == "$userId"),
            "Expected $userId in completions: {:?}",
            labels
        );
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_fragment_spread() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment MyFrag on User { id } query { users { ... } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions after "..."
    let position = Position::new(0, 50);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "MyFrag"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_types_in_fragment() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment MyFrag on  { id }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions at "on |"
    let position = Position::new(0, 19);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
        assert!(
            items.iter().any(|i| i.label == "User"),
            "Expected User in completions: {:?}",
            labels
        );
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_fragment_spread_acceptance() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment MyFrag on User { id }\nquery { users { ... } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions after "..." (which is at line 1, column 19)
    let position = Position::new(1, 19);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let item = items
            .iter()
            .find(|i| i.label == "MyFrag")
            .expect("MyFrag completion not found");

        // Apply completion
        let mut final_text = text.to_string();
        if let Some(edit) = &item.text_edit {
            match edit {
                CompletionTextEdit::Edit(text_edit) => {
                    // Manual application of text edit for test assertion
                    let lines: Vec<&str> = final_text.split('\n').collect();
                    let start_line = text_edit.range.start.line as usize;
                    let start_char = text_edit.range.start.character as usize;
                    let end_line = text_edit.range.end.line as usize;
                    let end_char = text_edit.range.end.character as usize;

                    let mut new_content = String::new();
                    for (i, line) in lines.iter().enumerate() {
                        if i < start_line {
                            new_content.push_str(line);
                            new_content.push('\n');
                        } else if i == start_line {
                            new_content.push_str(&line[..start_char]);
                            new_content.push_str(&text_edit.new_text);
                            if i == end_line {
                                new_content.push_str(&line[end_char..]);
                                if i < lines.len() - 1 {
                                    new_content.push('\n');
                                }
                            }
                        } else if i > end_line {
                            new_content.push_str(line);
                            if i < lines.len() - 1 {
                                new_content.push('\n');
                            }
                        } else if i == end_line {
                            new_content.push_str(&line[end_char..]);
                            if i < lines.len() - 1 {
                                new_content.push('\n');
                            }
                        }
                    }
                    final_text = new_content;
                }
                _ => panic!("Expected standard text edit"),
            }
        } else {
            // Fallback to insert_text or label
            let insert_text = item.insert_text.as_ref().unwrap_or(&item.label);
            let lines: Vec<&str> = final_text.split('\n').collect();
            let mut new_content = String::new();
            for (i, line) in lines.iter().enumerate() {
                if i == position.line as usize {
                    new_content.push_str(&line[..position.character as usize]);
                    new_content.push_str(insert_text);
                    new_content.push_str(&line[position.character as usize..]);
                } else {
                    new_content.push_str(line);
                }
                if i < lines.len() - 1 {
                    new_content.push('\n');
                }
            }
            final_text = new_content;
        }

        assert_eq!(
            final_text,
            "fragment MyFrag on User { id }\nquery { users { ...MyFrag } }"
        );
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_directives_on_field() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! } directive @testDirective on FIELD",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "query { users { id @ } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions at "@|"
    let position = Position::new(0, 20);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "testDirective"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_directives_on_fragment() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "fragment MyFrag on User @ { id }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions at "@|"
    let position = Position::new(0, 25);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "public"));
        assert!(items.iter().any(|i| i.label == "type_only"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_introspection_fields() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("test.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: dir.path().to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    let query_path = dir.path().join("test.graphql");
    let text = "query {  }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

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

    // Request completions at "query { | }" - should include __schema and __type
    let position = Position::new(0, 8);
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
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        // Should have regular fields
        assert!(
            items.iter().any(|i| i.label == "users"),
            "Should include regular field 'users'"
        );
        
        // Should have __typename (available on all types)
        assert!(
            items.iter().any(|i| i.label == "__typename"),
            "Should include __typename"
        );
        
        // Should have __schema (Query root only)
        let schema_item = items.iter().find(|i| i.label == "__schema");
        assert!(schema_item.is_some(), "Should include __schema on Query root");
        if let Some(item) = schema_item {
            assert_eq!(
                item.detail.as_deref(),
                Some("__Schema!"),
                "Should have correct type for __schema"
            );
        }
        
        // Should have __type (Query root only)
        let type_item = items.iter().find(|i| i.label == "__type");
        assert!(type_item.is_some(), "Should include __type on Query root");
        if let Some(item) = type_item {
            assert_eq!(
                item.detail.as_deref(),
                Some("__Type"),
                "Should have correct type for __type"
            );
        }
    } else {
        panic!("Expected array of completions");
    }

    // Now test that __schema and __type are NOT included on non-root types
    let text2 = "query { users {  } }";
    fs::write(&query_path, text2).unwrap();

    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text2.to_string(),
        }],
    };
    service
        .call(
            Request::build("textDocument/didChange")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // Request completions at "users { | }"
    let position = Position::new(0, 16);
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
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        // Should have regular User fields
        assert!(
            items.iter().any(|i| i.label == "id"),
            "Should include regular field 'id'"
        );
        assert!(
            items.iter().any(|i| i.label == "username"),
            "Should include regular field 'username'"
        );
        
        // Should still have __typename
        assert!(
            items.iter().any(|i| i.label == "__typename"),
            "Should include __typename on User type"
        );
        
        // Should NOT have __schema (not Query root)
        assert!(
            !items.iter().any(|i| i.label == "__schema"),
            "Should NOT include __schema on non-root User type"
        );
        
        // Should NOT have __type (not Query root)
        assert!(
            !items.iter().any(|i| i.label == "__type"),
            "Should NOT include __type on non-root User type"
        );
    } else {
        panic!("Expected array of completions");
    }
}
