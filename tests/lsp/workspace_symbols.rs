use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_workspace_symbols() {
    let (dir, config) = make_temp_project_with_schema("type Query { me: String }", "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open File A with symbol "UserFields"
    let text_a = "fragment UserFields on User { id }";
    let uri_a = write_project_file(&dir, "a.graphql", text_a);
    lsp_did_open(&mut service, uri_a.clone(), "graphql", 1, text_a).await;

    // 2. Open File B with symbol "GetMe"
    let text_b = "query GetMe { me }";
    let uri_b = write_project_file(&dir, "b.graphql", text_b);
    lsp_did_open(&mut service, uri_b.clone(), "graphql", 1, text_b).await;

    // 3. Search for "User"
    let params = WorkspaceSymbolParams {
        query: "User".to_string(),
        ..Default::default()
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    let symbols = result.expect("Expected symbols");
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UserFields" && s.location.uri == uri_a)
    );
    assert!(!symbols.iter().any(|s| s.name == "GetMe"));

    // 4. Search for "Me"
    let params = WorkspaceSymbolParams {
        query: "Me".to_string(),
        ..Default::default()
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    let symbols = result.expect("Expected symbols");
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "GetMe" && s.location.uri == uri_b)
    );
}
