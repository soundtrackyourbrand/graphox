use tower_lsp::lsp_types::*;
use graphql_rust::{Backend, Config, config::ProjectConfig, config::SchemaSource, config::GlobPattern};
use tower_lsp::LspService;
use tower_service::Service;
use tower_lsp::jsonrpc::Request;
use std::path::Path;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_lsp_multi_schema_support() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    // 1. Create base.graphql
    let base_path = base_dir.join("base.graphql");
    fs::write(&base_path, "type User { id: ID! name: String } type Query { me: User }").unwrap();

    // 2. Create ext.graphql
    let ext_path = base_dir.join("ext.graphql");
    fs::write(&ext_path, "extend type User { email: String }").unwrap();

    // 3. Create query.graphql
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name email } }";
    fs::write(&query_path, query_text).unwrap();

    // 4. Create Config
    let config = Config {
        output_dir: None,
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Multiple(vec!["base.graphql".to_string(), "ext.graphql".to_string()]),
                include: GlobPattern::Single("query.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
            }
        ],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        base_dir: base_dir.to_path_buf(),
    };

    let (mut service, _) = LspService::new(|client| {
        Backend::new(client, Some(config), base_path.to_str().unwrap())
    });

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

    let query_uri = Url::from_file_path(&query_path).unwrap();

    // Open document
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    };
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(&params).unwrap()).finish()).await.unwrap();

    // 5. Test Completion for 'email' (from ext.graphql)
    // query { me { id name e| } }
    let position = Position::new(0, 23); 
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let request = Request::build("textDocument/completion").id(1).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "email"), "Should suggest 'email' from extension schema");
        assert!(items.iter().any(|i| i.label == "name"), "Should suggest 'name' from base schema");
    } else {
        panic!("Expected completions");
    }

    // 6. Test Hover for 'email'
    let position = Position::new(0, 23);
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
    };
    let request = Request::build("textDocument/hover").id(2).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    
    assert!(result.is_some());
    let hover = result.unwrap();
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(markup.value.contains("String"), "Hover should show type from extension schema");
    }

    // 7. Test Go to Definition for 'id' (from base.graphql)
    let position = Position::new(0, 15); // on 'id'
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let request = Request::build("textDocument/definition").id(3).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let _result: Option<GotoDefinitionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    // 8. Test Fragments and @public check
    // Create frag.graphql
    let frag_path = base_dir.join("frag.graphql");
    let frag_text = "fragment UserFields on User { id name }";
    fs::write(&frag_path, frag_text).unwrap();
    let frag_uri = Url::from_file_path(&frag_path).unwrap();

    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: frag_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: frag_text.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    // Use fragment in query
    let query_text_2 = "query { me { ...UserFields } }";
    service.call(Request::build("textDocument/didChange").params(serde_json::to_value(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri: query_uri.clone(), version: 2 },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: query_text_2.to_string(),
        }],
    }).unwrap()).finish()).await.unwrap();

    // Goto Definition for UserFields
    let position = Position::new(0, 18); 
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let request = Request::build("textDocument/definition").id(4).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<GotoDefinitionResponse> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert_eq!(loc.uri, frag_uri, "Should jump to UserFields definition");
        }
        _ => panic!("Fragment definition not found"),
    }
}
