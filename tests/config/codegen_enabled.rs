use graphox::Config;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_global_codegen_false_inherited_by_projects() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
codegen: false
projects:
  - include: "src/**/*.graphql"
    schema: "schema.graphql"
"#;
    fs::write(&config_path, config_content).unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();

    let result = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let project = &result.projects()[0];

    // This is what currently fails based on my analysis
    assert!(
        !result.get_project_codegen_enabled(project),
        "Project should inherit global codegen: false"
    );
}

#[test]
fn test_project_can_override_global_codegen_false() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
codegen: false
projects:
  - include: "src/**/*.graphql"
    schema: "schema.graphql"
    codegen: true
"#;
    fs::write(&config_path, config_content).unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();

    let result = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let project = &result.projects()[0];

    assert!(
        result.get_project_codegen_enabled(project),
        "Project should be able to override global codegen: false"
    );
}

#[test]
fn test_global_codegen_true_inherited_by_projects() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
codegen: true
projects:
  - include: "src/**/*.graphql"
    schema: "schema.graphql"
"#;
    fs::write(&config_path, config_content).unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();

    let result = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let project = &result.projects()[0];

    assert!(
        result.get_project_codegen_enabled(project),
        "Project should inherit global codegen: true"
    );
}

#[test]
fn test_project_can_override_global_codegen_true() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
codegen: true
projects:
  - include: "src/**/*.graphql"
    schema: "schema.graphql"
    codegen: false
"#;
    fs::write(&config_path, config_content).unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();

    let result = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let project = &result.projects()[0];

    assert!(
        !result.get_project_codegen_enabled(project),
        "Project should be able to override global codegen: true"
    );
}

#[test]
fn test_global_codegen_false_affects_schema_types() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
codegen: false
schema_types:
  - schema: "schema.graphql"
    output: "schema.ts"
"#;
    fs::write(&config_path, config_content).unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { a: String }",
    )
    .unwrap();

    let result = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();

    // We need a way to check if schema types should be generated.
    // Since there is no per-schema-type enabled flag yet, we check the global one.
    assert!(
        !result.codegen().is_enabled(),
        "Global codegen should be disabled"
    );
}

#[test]
fn test_lsp_automatic_codegen_respects_global_codegen_false() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("graphox.yaml");
    let config_content = r#"
codegen: false
"#;
    fs::write(&config_path, config_content).unwrap();

    let result = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();

    assert!(
        !result.lsp_automatic_codegen(),
        "LSP automatic codegen should be disabled when global codegen is false"
    );
}
