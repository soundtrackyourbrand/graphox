use tower_lsp::lsp_types::*;
use graphql_rust::{Backend, DocumentState};
use apollo_compiler::Schema;
use tower_lsp::{LanguageServer, LspService};
use tower_service::Service;
use tower_lsp::jsonrpc::Request;

#[tokio::test]
async fn test_completion_fields() {
    let (mut service, _) = LspService::new(|client| Backend::new(client, None, "tests/fixtures/simple_schema.graphql"));

    // 0. Initialize
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

    // 1. Open file
    let uri = Url::parse("file:///completion.graphql").unwrap();
    let text = r#"
        query GetUser {
            users {
                i
            }
        }
    "#;
    // Cursor after 'i' -> expect 'id'
    
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

    // 2. Request completion
    // Line 3: "                i"
    // Indent 16 spaces. 'i' at 16. Cursor at 17?
    let position = Position::new(3, 17);
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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => {
            let has_id = items.iter().any(|item| item.label == "id");
            let has_email = items.iter().any(|item| item.label == "email");
            let has_username = items.iter().any(|item| item.label == "username");
            
            assert!(has_id, "Should suggest 'id'");
            assert!(has_email, "Should suggest 'email'");
            assert!(has_username, "Should suggest 'username'");
        }
        _ => panic!("Expected Array of completions, got {:?}", result),
    }
}

#[tokio::test]
async fn test_completion_types_in_fragment() {
    let (mut service, _) = LspService::new(|client| Backend::new(client, None, "tests/fixtures/simple_schema.graphql"));
    // Initialize ... (omitted for brevity, shared setup would be nice but copy-paste is safer for now)
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

    let uri = Url::parse("file:///fragment_completion.graphql").unwrap();
    let text = r#"
        fragment F on U
    "#;
    // Cursor after 'U'
    
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

    let position = Position::new(1, 23); // "        fragment F on U" -> 8 + "fragment F on ".len() (14) + "U".len() (1) = 23.
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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => {
            let has_user = items.iter().any(|item| item.label == "User");
            let has_post = items.iter().any(|item| item.label == "Post");
            
            assert!(has_user, "Should suggest 'User'");
            assert!(has_post, "Should suggest 'Post'");
        }
        _ => panic!("Expected Array of completions, got {:?}", result),
    }
}

#[tokio::test]
async fn test_completion_fragment_spread() {
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

    // 1. Open file with a fragment and a spread
    let uri = Url::parse("file:///spread_completion.graphql").unwrap();
    let text = r#"
        fragment UserFields on User {
            id
            username
        }

        query GetUser {
            users {
                ...U
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

    // 2. Request completion after '...U'
    // Line 8: "                ...U"
    // 16 spaces + "...".len() (3) + "U".len() (1) = 20.
    let position = Position::new(8, 20);
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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => {
            let has_fragment = items.iter().any(|item| item.label == "UserFields");
            assert!(has_fragment, "Should suggest 'UserFields' fragment");
        }
        _ => panic!("Expected Array of completions, got {:?}", result),
    }
}

#[tokio::test]
async fn test_completion_variables() {
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

    // 1. Open file with a variable definition and usage
    let uri = Url::parse("file:///variable_completion.graphql").unwrap();
    let text = r#"
        query GetNode($nodeId: ID!) {
            node(id: $n) {
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

    // 2. Request completion after '$n'
    // Line 2: "            node(id: $n) {"
    // 12 spaces + "node(id: ".len() (9) + "$".len() (1) = 22.
    // Let's try cursor at 22 (after $)
    let position = Position::new(2, 22);
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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => {
            let has_variable = items.iter().any(|item| item.label == "$nodeId");
            assert!(has_variable, "Should suggest '$nodeId' variable, got: {:?}", items);
        }
        _ => panic!("Expected Array of completions, got {:?}", result),
    }
}
