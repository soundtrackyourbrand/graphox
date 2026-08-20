#![allow(unused_imports)]

use graphox::Config;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
#[ntest::timeout(3000)]
fn test_config_invalid_yaml() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    fs::write(&config_path, "invalid: yaml: content: [").unwrap();

    let result = Config::load_from_dir(temp_dir.path());
    assert!(result.is_err(), "Should fail to load invalid YAML config");
}

#[test]
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
fn test_config_empty_base_dir() {
    let result = Config::load_from_dir(Path::new("/nonexistent/path"));
    assert!(
        result.unwrap().is_none(),
        "Should fail with nonexistent base directory"
    );
}

#[test]
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
    assert_eq!(config.projects().len(), 2, "Should have two projects");
}

#[test]
#[ntest::timeout(3000)]
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
    assert!(config.enable_schema_cache());
    assert!(config.lsp_automatic_codegen());
}

#[test]
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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

#[test]
#[ntest::timeout(3000)]
fn test_codegen_react_apollo_hooks_config_parsing() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let schema_path = temp_dir.path().join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let config_content = r#"
        codegen:
          react_apollo_hooks: true
          apolloReactCommonImportFrom: "@apollo/client/react"
          apolloReactHooksImportFrom: "@apollo/client/react"
        projects:
          - name: test
            schema: schema.graphql
            include: "*.graphql"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let codegen = config.codegen();

    assert!(codegen.react_apollo_hooks());
    assert_eq!(
        codegen.apollo_react_common_import_from(),
        "@apollo/client/react"
    );
    assert_eq!(
        codegen.apollo_react_hooks_import_from(),
        "@apollo/client/react"
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_project_codegen_react_apollo_hooks_override() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let schema_path = temp_dir.path().join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let config_content = r#"
        codegen:
          react_apollo_hooks: false
          apolloReactCommonImportFrom: "@apollo/client"
        projects:
          - name: test
            schema: schema.graphql
            include: "*.graphql"
            codegen:
              react_apollo_hooks: true
              apolloReactHooksImportFrom: "@apollo/client/react"
    "#;
    fs::write(&config_path, config_content).unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let codegen = config.get_codegen_config(Some(&config.projects()[0]));

    assert!(codegen.react_apollo_hooks());
    assert_eq!(codegen.apollo_react_common_import_from(), "@apollo/client");
    assert_eq!(
        codegen.apollo_react_hooks_import_from(),
        "@apollo/client/react"
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_project_codegen_entrypoint_name_override() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let schema_path = temp_dir.path().join("schema.graphql");
    fs::write(&schema_path, "type Query { user: String }").unwrap();

    let config_content = r#"
        codegen:
          entrypoint_name: graphql
        projects:
          - name: test
            schema: schema.graphql
            include: "*.graphql"
            codegen:
              entrypoint_name: Queries
    "#;
    fs::write(&config_path, config_content).unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let codegen = config.get_codegen_config(Some(&config.projects()[0]));

    assert_eq!(codegen.entrypoint_name(), "Queries");
}

#[test]
#[ntest::timeout(3000)]
fn test_allow_no_documents_defaults_to_false() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("graphox.yaml"),
        r#"
projects:
  - schema: schema.graphql
    documents: "**/*.graphql"
"#,
    )
    .unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    assert!(!config.allow_no_documents());
    assert!(!config.get_project_allow_no_documents(&config.projects()[0]));
}

#[test]
#[ntest::timeout(3000)]
fn test_allow_no_documents_global_and_per_project_override() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("graphox.yaml"),
        r#"
allow_no_documents: true

projects:
  - schema: schema.graphql
    documents: "a/**/*.graphql"
  - schema: schema.graphql
    documents: "b/**/*.graphql"
    allow_no_documents: false
"#,
    )
    .unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    assert!(config.allow_no_documents(), "global should be true");
    assert!(
        config.get_project_allow_no_documents(&config.projects()[0]),
        "project without an override should inherit the global"
    );
    assert!(
        !config.get_project_allow_no_documents(&config.projects()[1]),
        "per-project false should override a global true"
    );
}
