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
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
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
        }],
        base_dir: dir.to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    }
}

#[tokio::test]
async fn test_hover_fragment_spread() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
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

    // 1. Open file with fragment and spread
    let query_path = dir.path().join("hover.graphql");
    let text = r#"
        fragment UserFields on User {
            id
            username
        }

        query {
            users {
                ...UserFields
            }
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // 2. Request hover over 'UserFields' in the spread
    let position = Position::new(8, 20);
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

    assert!(result.is_some(), "Hover should return something");
    let hover = result.unwrap();
    match hover.contents {
        HoverContents::Markup(m) => {
            assert!(
                m.value.contains("UserFields"),
                "Hover content should contain fragment name"
            );
            assert!(
                m.value.contains("id"),
                "Hover content should contain fragment fields"
            );
            assert!(
                m.value.contains("username"),
                "Hover content should contain fragment fields"
            );
        }
        _ => panic!("Expected Markup hover contents"),
    }
}

#[tokio::test]
async fn test_hover_schema_type() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
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

    // 1. Open file
    let query_path = dir.path().join("hover_schema.graphql");
    let text = r#"
        query {
            users {
                id
            }
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let position = Position::new(2, 13);
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

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(m.value.contains("User"), "Should show type info for User");
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_graphql_description() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        r#"
        "This is a documented type"
        type DocumentedType {
            id: ID!
        }
        type Query { someField(arg: DocumentedType): ID }
    "#,
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

    let query_path = dir.path().join("hover_desc.graphql");
    let text = r#"
        query {
            someField(arg: { id: "1" }): ID
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Hover over 'someField' to see documentation of its argument type or field
    // Actually the previous test hovered over type definition.
    // Let's just open the schema itself.
    let schema_uri = Url::from_file_path(&schema_path).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
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

    let position = Position::new(2, 15); // "type DocumentedType"
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_uri.clone(),
            },
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

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("This is a documented type"),
            "Should show documentation description"
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_schema_field() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
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

    // 1. Open file
    let query_path = dir.path().join("hover_field.graphql");
    let text = r#"
        query {
            users {
                id
            }
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let position = Position::new(3, 17);
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

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field User.id"),
            "Should show field info for User.id"
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should show correct field type"
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_variable() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
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

    // 1. Open file with variable
    let query_path = dir.path().join("hover_var.graphql");
    let text = r#"
        query GetUser($id: ID!) {
            users {
                id
            }
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Hover over '$id' in the variable definition
    let position = Position::new(1, 22); // $id: ID!
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
        "Hover should return something for variable definition"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("variable $id"),
            "Should contain variable name"
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should contain variable type"
        );
    } else {
        panic!("Expected Markup contents");
    }

    // Hover over its usage
    let text_with_usage = r#"
        query GetUser($id: ID!) {
            node(id: $id) {
                id
            }
        }
    "#;
    fs::write(dir.path().join("hover_var_usage.graphql"), text_with_usage).unwrap();
    let query_path_usage =
        std::fs::canonicalize(dir.path().join("hover_var_usage.graphql")).unwrap();
    let uri_usage = Url::from_file_path(&query_path_usage).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri_usage.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text_with_usage.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let position = Position::new(2, 22); // $id in node(id: $id)
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: uri_usage.clone(),
            },
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
        "Hover should return something for variable usage"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("variable $id"),
            "Should contain variable name"
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should contain variable type"
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_argument() {
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
            include: GlobPattern::Single("**/*.graphql".to_string()),
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

    // 1. Open file
    let query_path = dir.path().join("hover_arg.graphql");
    let text = r#"
        query {
            user(id: "1") {
                id
            }
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let position = Position::new(2, 17); // "id" in "user(id: \"1\")"
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
        "Hover should return something for argument 'id'"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("argument id"),
            "Should show argument info for 'id', got: {}",
            m.value
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should show correct argument type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_input_object_field() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { createUser(input: CreateUserInput!): User } \
         input CreateUserInput { username: String! age: Int } \
         type User { id: ID! username: String! }",
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

    // 1. Open file
    let query_path = dir.path().join("hover_input.graphql");
    let text = r#"
        query {
            createUser(input: { username: "emma", age: 25 }) {
                id
            }
        }
    "#;
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
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Hover over 'username' in the input object
    let position = Position::new(2, 35); // "username" in "input: { username: \"emma\""
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
        "Hover should return something for input field 'username'"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field CreateUserInput.username"),
            "Should show field info, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Type: `String!`"),
            "Should show correct field type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }

    // Hover over 'age'
    let position = Position::new(2, 52); // "age" in "age: 25"
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
        "Hover should return something for input field 'age'"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field CreateUserInput.age"),
            "Should show field info, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Type: `Int`"),
            "Should show correct field type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_builtin_typename() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

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

    let query_path = dir.path().join("hover_typename.graphql");
    let text = r#"
        query {
            users {
                __typename
            }
        }
    "#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(&query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let position = Position::new(3, 20);
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

    let hover = result.expect("hover should succeed");
    if let HoverContents::Markup(m) = hover.contents {
        assert!(
            m.value.contains("field User.__typename"),
            "Should show typename field info, got: {}",
            m.value
        );
        assert!(
            m.value.contains("The GraphQL type name of the current selection."),
            "Should show fallback description for builtin field, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_builtin_schema_fields() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

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

    let query_path = dir.path().join("hover_schema_fields.graphql");
    let text = r#"
        query {
            __schema {
                types {
                    name
                }
            }
            __type(name: "User") {
                name
            }
        }
    "#;
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(&query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let schema_position = Position::new(2, 14);
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: schema_position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/hover")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let hover = result.expect("hover should succeed for __schema");
    if let HoverContents::Markup(m) = hover.contents {
        assert!(
            m.value.contains("field Query.__schema"),
            "Should describe __schema, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Access the current schema introspection object."),
            "Should show fallback description for __schema, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }

    let type_position = Position::new(7, 14);
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: type_position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/hover")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let hover = result.expect("hover should succeed for __type");
    if let HoverContents::Markup(m) = hover.contents {
        assert!(
            m.value.contains("field Query.__type"),
            "Should describe __type, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Look up a type definition by its name."),
            "Should show fallback description for __type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}
