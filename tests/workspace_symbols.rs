use tower_lsp::lsp_types::*;
use graphql_rust::{Backend, Config, config::{ProjectConfig, SchemaSource, GlobPattern}};
use tower_lsp::LspService;
use tower_service::Service;
use tower_lsp::jsonrpc::Request;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_workspace_symbols() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
        }],
        base_dir: base_dir.to_path_buf(),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap().unwrap();
    service.call(Request::build("initialized").params(serde_json::json!({})).finish()).await.unwrap();

    // 1. Open File A with symbol "UserFields"
    let path_a = base_dir.join("a.graphql");
    let text_a = "fragment UserFields on User { id }";
    fs::write(&path_a, text_a).unwrap();
    let uri_a = Url::from_file_path(&path_a).unwrap();

    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri_a.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text_a.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    // 2. Open File B with symbol "GetMe"
    let path_b = base_dir.join("b.graphql");
    let text_b = "query GetMe { me }";
    fs::write(&path_b, text_b).unwrap();
    let uri_b = Url::from_file_path(&path_b).unwrap();

    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri_b.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text_b.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    // 3. Search for "User"
    let params = WorkspaceSymbolParams {
        query: "User".to_string(),
        ..Default::default()
    };
    
    let request = Request::build("workspace/symbol").id(1).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<SymbolInformation>> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    
    let symbols = result.expect("Expected symbols");
    assert!(symbols.iter().any(|s| s.name == "UserFields" && s.location.uri == uri_a));
    assert!(!symbols.iter().any(|s| s.name == "GetMe"));

    // 4. Search for "Me"
    let params = WorkspaceSymbolParams {
        query: "Me".to_string(),
        ..Default::default()
    };
    
    let request = Request::build("workspace/symbol").id(2).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<SymbolInformation>> = serde_json::from_value(response.result().unwrap().clone()).unwrap();
    
    let symbols = result.expect("Expected symbols");
    assert!(symbols.iter().any(|s| s.name == "GetMe" && s.location.uri == uri_b));
}
