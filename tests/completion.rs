use tower_lsp::lsp_types::*;
use graphql_rust::Backend;
use tower_lsp::LspService;
use tower_service::Service;
use tower_lsp::jsonrpc::Request;

#[tokio::test]
async fn test_completion_fields() {
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

    let uri = Url::parse("file:///test.graphql").unwrap();
    let text = "query { users {  } }";
    
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "id"));
        assert!(items.iter().any(|i| i.label == "username"));
    } else {
        panic!("Expected array of completions");
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

    // Use a REAL file path to avoid "source file not found" if tree-sitter or other parts try to read it
    let uri = Url::from_file_path(std::fs::canonicalize("tests/fixtures/simple_schema.graphql").unwrap()).unwrap();
    let text = "query GetUser($userId: ID!) { user(id: $) }";
    
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
        assert!(items.iter().any(|i| i.label == "$userId"), "Expected $userId in completions: {:?}", labels);
    } else {
        panic!("Expected array of completions");
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

    let uri = Url::parse("file:///test.graphql").unwrap();
    let text = "fragment MyFrag on User { id } query { users { ... } }";
    
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "MyFrag"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_types_in_fragment() {
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

    let uri = Url::parse("file:///test.graphql").unwrap();
    let text = "fragment MyFrag on  { id }";
    
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

    // Request completions at "on |"
    // text is "fragment MyFrag on  { id }"
    // index of space after on is 18.
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
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
        assert!(items.iter().any(|i| i.label == "User"), "Expected User in completions: {:?}", labels);
    } else {
        panic!("Expected array of completions");
    }
}
