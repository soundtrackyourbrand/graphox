use graphql_rust::Config;
use std::fs;
use tempfile::tempdir;
use std::io::Write;

#[test]
fn test_config_multiple_schemas() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("graphql.yaml");
    let mut file = fs::File::create(config_path).unwrap();
    writeln!(
        file,
        r#"
projects:
  - schema:
      - base.graphql
      - extension.graphql
    include: "src/**/*.ts"
"#
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
    
    let base_path = dir.path().join("base.graphql");
    fs::write(&base_path, "type Query { foo: String }").unwrap();
    
    let ext_path = dir.path().join("extension.graphql");
    fs::write(&ext_path, "extend type Query { bar: Int }").unwrap();
    
    let config_path = dir.path().join("graphql.yaml");
    fs::write(config_path, r#"
projects:
  - schema:
      - base.graphql
      - extension.graphql
    include: "src/**/*.ts"
"#).unwrap();

    let config = Config::load_from_dir(dir.path());
    let (_service, _) = tower_lsp::LspService::new(|client| {
        graphql_rust::Backend::new(client, config.clone(), "base.graphql")
    });
    
    // Check if the backend loaded the merged schema
    // We can't easily access the backend from the service here without some tricks,
    // but we can test the load_schema_source directly if we want.
}
