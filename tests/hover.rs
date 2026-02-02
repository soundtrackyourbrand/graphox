use tower_lsp::lsp_types::*;
use graphql_rust::Backend;
use tower_lsp::LspService;
use tower_service::Service;
use tower_lsp::jsonrpc::Request;

#[tokio::test]
async fn test_hover_fragment_spread() {
    let (mut service, _) = LspService::new(|client| Backend::new(client, None, "tests/fixtures/simple_schema.graphql"));

    // Initialize
    let init_params = InitializeParams { ..Default::default() };
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
    let uri = Url::parse("file:///hover.graphql").unwrap();
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
    // Line 8: "                ...UserFields"
    // 16 spaces + 3 dots = 19. 'UserFields' starts at 19.
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
    let (mut service, _) = LspService::new(|client| {
        Backend::new(client, None, "tests/fixtures/simple_schema.graphql")
    });

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
    let uri = Url::parse("file:///hover_schema.graphql").unwrap();
    let text = r#"
        query {
            users {
                id
            }
        }
    "#;

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

    // 2. Request hover over 'users' which is a field returning [User!]
    // Line 2: "            users {"
    // 12 spaces. 'users' starts at 12.
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
    let result: Option<Hover> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(m.value.contains("User"), "Should show type info for User");
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_graphql_description() {
    let (mut service, _) = LspService::new(|client| {
        Backend::new(client, None, "tests/fixtures/simple_schema.graphql")
    });

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

    // 1. Open file with a documented type
    let uri = Url::parse("file:///hover_desc.graphql").unwrap();
    let text = r#"
        "This is a documented type"
        type DocumentedType {
            id: ID!
        }

        query {
            someField(arg: DocumentedType): ID
        }
    "#;

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

    // 2. Request hover over 'DocumentedType' in the definition
    let position = Position::new(2, 15);
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
    let result: Option<Hover> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

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
    let (mut service, _) = LspService::new(|client| {
        Backend::new(client, None, "tests/fixtures/simple_schema.graphql")
    });

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
    let uri = Url::parse("file:///hover_field.graphql").unwrap();
    let text = r#"
        query {
            users {
                id
            }
        }
    "#;

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

    // 2. Request hover over 'id' which is a field of User
    // Line 3: "                id"
    // 16 spaces. 'id' starts at 16.
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
    let result: Option<Hover> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(m.value.contains("field User.id"), "Should show field info for User.id");
        assert!(m.value.contains("Type: `ID!`"), "Should show correct field type");
    } else {
        panic!("Expected Markup contents");
    }
}
