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
async fn test_goto_definition_cross_file() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    // Create package.json to define a package root
    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

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

    // 0. Initialize
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

    // 1. Create and Open the fragment definition file
    let frag_path = base_dir.join("user_fragment.graphql");
    let fragment_text = "fragment UserFields on User { id name }";
    fs::write(&frag_path, fragment_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let fragment_uri = Url::from_file_path(&frag_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: fragment_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: fragment_text.to_string(),
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

    // 2. Create and Open the query file that uses the fragment
    let query_path = base_dir.join("query_with_fragment.graphql");
    let query_text = "query GetUser { user { ...UserFields } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
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

    // 3. Trigger Go to Definition on "...UserFields" in query file
    let position = Position::new(0, 26);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
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
    if let Some(err) = response.error() {
        panic!("LSP Error: {:?}", err);
    }
    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, fragment_uri);
            assert_eq!(location.range.start.line, 0);
            assert_eq!(location.range.start.character, 9);
        }
        _ => panic!("Expected Scalar location, got {:?}", result),
    }
}
