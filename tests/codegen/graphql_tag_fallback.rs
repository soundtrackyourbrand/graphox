use std::process::Command;

fn setup_codegen_fixture(graphql_tag_fallback: bool) -> (String, std::process::Output) {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir_handle = tempfile::tempdir().unwrap();
    let temp_dir = temp_dir_handle.path();

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

    // Create config
    let config = if graphql_tag_fallback {
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "."
    codegen:
      graphql_tag_fallback: true
"#
    } else {
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "."
"#
    };

    std::fs::write(temp_dir.join("graphox.yaml"), config).unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(temp_dir)
        .output()
        .expect("Failed to execute process");

    let entrypoint_file = temp_dir.join("graphql.ts");
    let content = if entrypoint_file.exists() {
        std::fs::read_to_string(&entrypoint_file).unwrap()
    } else {
        String::new()
    };

    (content, output)
}

#[test]
#[ntest::timeout(2000)]
fn test_graphql_tag_fallback_enabled() {
    let (content, output) = setup_codegen_fixture(true);

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify graphql-tag import
    assert!(
        content.contains("import gqlTag from \"graphql-tag\";"),
        "Missing graphql-tag import. Content:\n{}",
        content
    );

    // Verify memoized fallback logic in graphql function
    assert!(
        content.contains(
            "return documents[source] || (documents[source] = gqlTag(withFragmentDefinitions(source)));"
        ),
        "Missing memoized gqlTag fallback logic in graphql function. Content:\n{}",
        content
    );
}

#[test]
#[ntest::timeout(2000)]
fn test_graphql_tag_fallback_disabled() {
    let (content, output) = setup_codegen_fixture(false);

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

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
}

#[test]
#[ntest::timeout(2000)]
fn test_graphql_tag_fallback_includes_fragment_sources_for_spreads() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir_handle = tempfile::tempdir().unwrap();
    let temp_dir = temp_dir_handle.path();

    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type Query { user(id: ID!): User } type User { id: ID! name: String }",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("query.graphql"),
        "query GetUser($id: ID!) { user(id: $id) { ...UserFields } }",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("fragments.graphql"),
        "fragment UserFields on User { id ...UserName }\nfragment UserName on User { name }",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "*.graphql"
    output_dir: "."
    codegen:
      graphql_tag_fallback: true
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let content = std::fs::read_to_string(temp_dir.join("graphql.ts")).unwrap();
    assert!(
        content.contains("const fragmentSources: { [key: string]: string } = {"),
        "Fallback entrypoint should embed fragment source definitions. Content:\n{}",
        content
    );
    assert!(
        content.contains("\"UserFields\": \"fragment UserFields on User { id ...UserName }"),
        "Fallback entrypoint should include source text for the directly referenced fragment. Content:\n{}",
        content
    );
    assert!(
        content.contains("\"UserName\": \"fragment UserFields on User { id ...UserName }\\nfragment UserName on User { name }\""),
        "Fallback entrypoint should include source text covering transitive fragment spreads. Content:\n{}",
        content
    );
    assert!(
        content.contains("function withFragmentDefinitions(source: string): string"),
        "Fallback entrypoint should resolve fragment spreads before calling gqlTag. Content:\n{}",
        content
    );
    assert!(
        content.contains("gqlTag(withFragmentDefinitions(source))"),
        "Fallback entrypoint should call gqlTag with fragment-enriched source. Content:\n{}",
        content
    );
}
