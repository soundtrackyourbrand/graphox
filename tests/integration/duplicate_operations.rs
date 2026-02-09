#![allow(unused_imports)]
use graphox::{
    Config,
    config::{GlobPattern, ProjectConfig, RulesConfig, SchemaSource},
    engine::Engine,
};
use std::fs;

use crate::support::create_doc;
use tower_lsp::lsp_types::Url;

#[tokio::test]
#[ntest::timeout(500)]
async fn test_document_operations_extraction() {
    let query_text = "query GetUser { user(id: \"1\") { id name } }";
    let uri = Url::parse("file:///test/query.graphql").unwrap();

    let doc = create_doc(uri.as_str(), query_text);

    assert_eq!(doc.operations().len(), 1, "Should extract 1 operation");
    assert_eq!(
        doc.operations()[0].name.as_ref().map(|s| s.as_ref()),
        Some("GetUser")
    );
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_check_command_duplicate_operations() {
    // Use LspTestScenario to create a temporary project layout with schema
    // and two query files that have duplicate operation names.
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
        )
        .with_file(
            "query1.graphql",
            "query GetUser { user(id: \"1\") { id name } }",
        )
        .with_file("query2.graphql", "query GetUser { user(id: \"2\") { id } }");

    let base_dir = scenario.write_files().unwrap();

    let config = Config {
        base_dir: base_dir.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            ..Default::default()
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    // Scan workspace to build operation index
    let workspace_metadata = Engine::scan_workspace(&config);

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
#[ntest::timeout(500)]
async fn test_check_command_unique_operations() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
        )
        .with_file(
            "query1.graphql",
            "query GetUser { user(id: \"1\") { id name } }",
        )
        .with_file(
            "query2.graphql",
            "query GetUserById { user(id: \"2\") { id } }",
        );

    let base_dir = scenario.write_files().unwrap();

    let config = Config {
        base_dir: base_dir.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            ..Default::default()
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let workspace_metadata = Engine::scan_workspace(&config);

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
