use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
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

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_workspace_symbols_filtering() {
    let (dir, config) =
        make_temp_project_with_schema("type Query { me: String user: String }", "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text_a = "query GetUser { user }";
    let uri_a = write_project_file(&dir, "a.graphql", text_a);
    lsp_did_open(&mut service, uri_a.clone(), "graphql", 1, text_a).await;

    let text_b = "query GetMe { me }";
    let uri_b = write_project_file(&dir, "b.graphql", text_b);
    lsp_did_open(&mut service, uri_b.clone(), "graphql", 1, text_b).await;

    let text_c = "fragment UserFields on User { id }";
    let uri_c = write_project_file(&dir, "c.graphql", text_c);
    lsp_did_open(&mut service, uri_c.clone(), "graphql", 1, text_c).await;

    let params = WorkspaceSymbolParams {
        query: "Get".to_string(),
        ..Default::default()
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    let symbols = result.expect("Expected symbols");
    let get_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| s.name.starts_with("Get"))
        .collect();

    assert_eq!(get_symbols.len(), 2);
    assert!(get_symbols.iter().any(|s| s.name == "GetUser"));
    assert!(get_symbols.iter().any(|s| s.name == "GetMe"));
    assert!(!get_symbols.iter().any(|s| s.name == "UserFields"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_workspace_symbols_large_workspace() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { me: String user: String post: String comment: String }",
        "**/*.graphql",
    );
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let files: Vec<(&str, &str)> = vec![
        ("q1.graphql", "query GetUser1 { user }"),
        ("q2.graphql", "query GetUser2 { user }"),
        ("q3.graphql", "query GetUser3 { user }"),
        ("q4.graphql", "query GetUser4 { user }"),
        ("q5.graphql", "query GetUser5 { user }"),
        ("f1.graphql", "fragment Frag1 on User { id }"),
        ("f2.graphql", "fragment Frag2 on User { id }"),
        ("f3.graphql", "fragment Frag3 on User { id }"),
    ];

    for (filename, content) in &files {
        let uri = write_project_file(&dir, filename, content);
        lsp_did_open(&mut service, uri, "graphql", 1, content).await;
    }

    let params = WorkspaceSymbolParams {
        query: "Get".to_string(),
        ..Default::default()
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    let symbols = result.expect("Expected symbols");
    assert!(
        symbols.len() >= 5,
        "Should find all Get* queries in workspace"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_workspace_symbols_fragment_only() {
    let (dir, config) = make_temp_project_with_schema("type Query { me: String }", "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text_a = "query GetUser { user }";
    let _uri_a = write_project_file(&dir, "a.graphql", text_a);
    lsp_did_open(&mut service, _uri_a.clone(), "graphql", 1, text_a).await;

    let text_b = "fragment UserFields on User { id name email }";
    let uri_b = write_project_file(&dir, "b.graphql", text_b);
    lsp_did_open(&mut service, uri_b.clone(), "graphql", 1, text_b).await;

    let params = WorkspaceSymbolParams {
        query: "UserFields".to_string(),
        ..Default::default()
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    let symbols = result.expect("Expected symbols");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "UserFields");
    assert_eq!(symbols[0].location.uri, uri_b);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_workspace_symbols_case_insensitive() {
    let (dir, config) = make_temp_project_with_schema("type Query { me: String }", "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text_a = "query GetUser { me }";
    let uri_a = write_project_file(&dir, "a.graphql", text_a);
    lsp_did_open(&mut service, uri_a.clone(), "graphql", 1, text_a).await;

    let params_lower = WorkspaceSymbolParams {
        query: "getuser".to_string(),
        ..Default::default()
    };

    let result_lower: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params_lower).await;

    let symbols_lower = result_lower.expect("Expected symbols");
    assert!(
        symbols_lower
            .iter()
            .any(|s| s.name.to_lowercase() == "getuser"),
        "Should find case-insensitive match"
    );

    let params_upper = WorkspaceSymbolParams {
        query: "GETUSER".to_string(),
        ..Default::default()
    };

    let result_upper: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params_upper).await;

    let symbols_upper = result_upper.expect("Expected symbols");
    assert!(
        symbols_upper
            .iter()
            .any(|s| s.name.to_uppercase() == "GETUSER"),
        "Should find case-insensitive match"
    );
}
