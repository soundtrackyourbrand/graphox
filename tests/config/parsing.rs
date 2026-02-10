#![allow(unused_imports)]

use graphox::Config;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn test_config_invalid_yaml() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    fs::write(&config_path, "invalid: yaml: content: [").unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(result.is_err(), "Should fail to load invalid YAML config");
}

#[test]
fn test_config_missing_schema() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
        projects:
          - name: test
            include: "**/*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(result.is_err(), "Should fail when schema file is missing");
}

#[test]
fn test_config_empty_base_dir() {
    let result = Config::load_from_dir(Path::new("/nonexistent/path"));
    assert!(
        result.unwrap().is_none(),
        "Should fail with nonexistent base directory"
    );
}

#[test]
fn test_config_valid_empty_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
        projects:
          - name: test
            schema: "schema.graphql"
            include: "*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let schema_path = temp_dir.path().join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(result.unwrap().is_some(), "Should load valid config");
}

#[test]
fn test_config_multiple_projects() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");

    let schema1_path = temp_dir.path().join("schema1.graphql");
    fs::write(&schema1_path, "type Query { user: String }").unwrap();

    let schema2_path = temp_dir.path().join("schema2.graphql");
    fs::write(&schema2_path, "type Query { post: String }").unwrap();

    let config_content = r#"
        projects:
          - name: project1
            schema: schema1.graphql
            include: "*.graphql"
          - name: project2
            schema: schema2.graphql
            include: "*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    let config = result.unwrap().unwrap();
    assert_eq!(config.projects.len(), 2, "Should have two projects");
}

#[test]
fn test_config_default_values() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let schema_path = temp_dir.path().join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let config_content = r#"
        projects:
          - name: test
            schema: schema.graphql
            include: "*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    let config = result.unwrap().unwrap();
    assert!(config.enable_schema_cache.is_none() || config.enable_schema_cache == Some(true));
    assert!(config.lsp_automatic_codegen.is_none() || config.lsp_automatic_codegen == Some(false));
}

#[test]
fn test_config_schema_as_list() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");

    let schema1_path = temp_dir.path().join("base.graphql");
    fs::write(&schema1_path, "type Query { a: String }").unwrap();

    let schema2_path = temp_dir.path().join("extended.graphql");
    fs::write(&schema2_path, "type Query { b: String }").unwrap();

    let config_content = r#"
        projects:
          - name: test
            schema:
              - base.graphql
              - extended.graphql
            include: "*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(
        result.unwrap().is_some(),
        "Should load config with schema list"
    );
}

#[test]
fn test_config_with_exclude_pattern() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let schema_path = temp_dir.path().join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let config_content = r#"
        projects:
          - name: test
            schema: schema.graphql
            include: "*.graphql"
            exclude:
              - "**/generated/**"
              - "**/backup/**"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(
        result.unwrap().is_some(),
        "Should load config with exclude patterns"
    );
}

#[test]
fn test_config_relative_schema_path() {
    let temp_dir = TempDir::new().unwrap();
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    let config_path = temp_dir.path().join("graphox.yaml");
    let schema_path = subdir.join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let config_content = r#"
        projects:
          - name: test
            schema: subdir/schema.graphql
            include: "*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(
        result.unwrap().is_some(),
        "Should load config with relative schema path"
    );
}
