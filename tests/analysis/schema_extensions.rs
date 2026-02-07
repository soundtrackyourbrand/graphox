use graphql_rust::Config;
use std::fs;
use tempfile::tempdir;

#[test]
#[ntest::timeout(100)]
fn test_config_multiple_schemas() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("graphql.yaml");
    fs::write(
        config_path,
        r#"
projects:
  - schema:
      - base.graphql
      - extension.graphql
    include: "src/**/*.ts"
"#,
    )
    .unwrap();

    let config = Config::load_from_dir(dir.path()).unwrap();
    assert_eq!(config.projects.len(), 1);
    let files = config.projects[0].schema.files();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], "base.graphql");
    assert_eq!(files[1], "extension.graphql");
}

#[tokio::test]
async fn test_multiple_schemas_loading() {
    let dir = tempdir().unwrap();

    fs::write(dir.path().join("base.graphql"), "type Query { foo: String }").unwrap();
    fs::write(
        dir.path().join("extension.graphql"),
        "extend type Query { bar: Int }",
    )
    .unwrap();

    let config_path = dir.path().join("graphql.yaml");
    fs::write(
        config_path,
        r#"
projects:
  - schema:
      - base.graphql
      - extension.graphql
    include: "src/**/*.ts"
"#,
    )
    .unwrap();

    let config = Config::load_from_dir(dir.path()).expect("Should load config");
    let (mut service, _) = crate::support::create_service(config);
    crate::support::lsp_initialize_sequence(&mut service).await;

    // Check if the backend loaded the merged schema via the backend instance
    let backend = service.inner();
    let config_read = backend.config.read().unwrap();
    assert_eq!(config_read.projects[0].schema.files().len(), 2);
}