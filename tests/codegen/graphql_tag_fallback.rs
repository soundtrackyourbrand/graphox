use std::process::Command;

#[test]
#[ntest::timeout(2000)]
fn test_graphql_tag_fallback_enabled() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_graphql_tag_fallback_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type Query { user(id: ID!): User } type User { id: ID! name: String }",
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(
        &query_file,
        "query GetUser($id: ID!) { user(id: $id) { id name } }",
    )
    .unwrap();

    // Create config with fallback enabled
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "."
    codegen:
      graphql_tag_fallback: true
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entrypoint_file = temp_dir.join("graphql.ts");
    assert!(entrypoint_file.exists(), "graphql.ts should exist");

    let content = std::fs::read_to_string(&entrypoint_file).unwrap();

    // Verify graphql-tag import
    assert!(
        content.contains("import gqlTag from \"graphql-tag\";"),
        "Missing graphql-tag import. Content:\n{}",
        content
    );

    // Verify fallback logic in graphql function
    assert!(
        content.contains("return documents[source] || gqlTag(source);"),
        "Missing gqlTag fallback logic in graphql function. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(2000)]
fn test_graphql_tag_fallback_disabled() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_graphql_tag_fallback_disabled_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type Query { user(id: ID!): User } type User { id: ID! name: String }",
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(
        &query_file,
        "query GetUser($id: ID!) { user(id: $id) { id name } }",
    )
    .unwrap();

    // Create config with fallback disabled (default)
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "."
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entrypoint_file = temp_dir.join("graphql.ts");
    assert!(entrypoint_file.exists(), "graphql.ts should exist");

    let content = std::fs::read_to_string(&entrypoint_file).unwrap();

    // Verify graphql-tag import is NOT present
    assert!(
        !content.contains("import gqlTag from \"graphql-tag\";"),
        "Should not have graphql-tag import. Content:\n{}",
        content
    );

    // Verify default fallback logic (empty object)
    assert!(
        content.contains("return documents[source] || {};"),
        "Missing default fallback logic in graphql function. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
