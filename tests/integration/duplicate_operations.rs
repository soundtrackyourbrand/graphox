use graphql_rust::{
    Config,
    config::{GlobPattern, ProjectConfig, RulesConfig, SchemaSource},
    engine::Engine,
};
use std::fs;
use tempfile::tempdir;

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_document_operations_extraction() {
    use graphql_rust::DocumentState;
    use tower_lsp::lsp_types::Url;

    let query_text = "query GetUser { user(id: \"1\") { id name } }";
    let uri = Url::parse("file:///test/query.graphql").unwrap();

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let doc = DocumentState::new(uri, query_text, parser);

    assert_eq!(doc.operations().len(), 1, "Should extract 1 operation");
    assert_eq!(doc.operations()[0].name, Some("GetUser".to_string()));
}

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_check_command_duplicate_operations() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
    )
    .unwrap();

    // Create two files with duplicate operation names
    fs::write(
        base_dir.join("query1.graphql"),
        "query GetUser { user(id: \"1\") { id name } }",
    )
    .unwrap();

    fs::write(
        base_dir.join("query2.graphql"),
        "query GetUser { user(id: \"2\") { id } }",
    )
    .unwrap();

    let config = Config {
        base_dir: base_dir.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    // Scan workspace to build operation index
    let workspace_metadata = Engine::scan_workspace(&config, |_, _| {});

    // Check that we found duplicate operations
    assert!(
        workspace_metadata
            .operation_names_by_project
            .contains_key("GetUser")
    );

    let get_user_locations = &workspace_metadata.operation_names_by_project["GetUser"];

    // Should have duplicates in project 0
    assert!(get_user_locations.contains_key(&0));
    let paths = &get_user_locations[&0];
    assert_eq!(paths.len(), 2, "Should find 2 files with GetUser operation");
}

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_check_command_unique_operations() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
    )
    .unwrap();

    // Create two files with DIFFERENT operation names
    fs::write(
        base_dir.join("query1.graphql"),
        "query GetUser { user(id: \"1\") { id name } }",
    )
    .unwrap();

    fs::write(
        base_dir.join("query2.graphql"),
        "query GetUserById { user(id: \"2\") { id } }",
    )
    .unwrap();

    let config = Config {
        base_dir: base_dir.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let workspace_metadata = Engine::scan_workspace(&config, |_, _| {});

    // Check that we found both operations
    assert!(
        workspace_metadata
            .operation_names_by_project
            .contains_key("GetUser")
    );
    assert!(
        workspace_metadata
            .operation_names_by_project
            .contains_key("GetUserById")
    );

    // Each should only appear once
    let get_user_locations = &workspace_metadata.operation_names_by_project["GetUser"];
    assert_eq!(get_user_locations[&0].len(), 1);

    let get_user_by_id_locations = &workspace_metadata.operation_names_by_project["GetUserById"];
    assert_eq!(get_user_by_id_locations[&0].len(), 1);
}
