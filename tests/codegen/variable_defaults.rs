use std::process::Command;

#[test]
fn test_codegen_variable_defaults() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_variable_defaults_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).expect("std::fs::remove_dir_all failed on &temp_dir");
    }
    std::fs::create_dir_all(&temp_dir).expect("std::fs::create_dir_all failed on &temp_dir");

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "scalar IsoCountry type Query { playlist(id: ID!, first: Int, after: String, market: IsoCountry): Playlist } type Playlist { id: ID! }",
    )
    .unwrap();

    // Create a query file with variable default value
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        r#"
const SourceEdit_PlaylistDoc = graphql(/* GraphQL */ `
  query SourceEdit_Playlist(
    $id: ID!
    $first: Int! = 2500
    $after: String
    $market: IsoCountry!
  ) {
    playlist(id: $id, first: $first, after: $after, market: $market) {
      id
    }
  }
`)
"#,
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
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

    let gen_file = temp_dir.join("query.codegen.ts");
    let content = std::fs::read_to_string(gen_file).unwrap();

    // Check for variables type
    // 'first' has a default value so it should be optional
    assert!(
        content.contains("first?: number;"),
        "Variable with default value should be optional. Content:\n{}",
        content
    );

    // 'id' and 'market' are Non-Null and have no default value, so they should remain required
    assert!(
        content.contains("id: string;"),
        "Non-default required variable 'id' should remain required. Content:\n{}",
        content
    );
    assert!(
        content.contains("market: any;"),
        "Non-default required variable 'market' should remain required. Content:\n{}",
        content
    );

    // 'after' is nullable, so it should be optional
    assert!(
        content.contains("after?: string | null;"),
        "Nullable variable 'after' should be optional. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).expect("std::fs::remove_dir_all failed on temp_dir");
}

#[test]
fn test_codegen_input_field_defaults() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_input_defaults_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).expect("std::fs::remove_dir_all failed on &temp_dir");
    }
    std::fs::create_dir_all(&temp_dir).expect("std::fs::create_dir_all failed on &temp_dir");

    // Create schema with input type having default value
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
input CreatePlaylistInput {
  name: String!
  public: Boolean! = false
  description: String
}

type Query {
  playlist(input: CreatePlaylistInput!): Playlist
}

type Playlist {
  id: ID!
}
"#,
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        "const q = graphql(`query GetPlaylist($input: CreatePlaylistInput!) { playlist(input: $input) { id } }`);",
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
    output_dir: "."
schema_types:
  - schema: "schema.graphql"
    output: "schema.ts"
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

    // Schema types are generated in schema.ts
    let schema_types_file = temp_dir.join("schema.ts");
    let content = std::fs::read_to_string(schema_types_file).unwrap();

    // Check for CreatePlaylistInput type
    // 'public' has a default value, so it should be optional
    assert!(
        content.contains("public?: boolean;"),
        "Input field with default value should be optional in schema types. Content:\n{}",
        content
    );

    // 'name' is Non-Null and has no default value, so it should remain required
    assert!(
        content.contains("name: string;"),
        "Non-default required input field 'name' should remain required. Content:\n{}",
        content
    );

    // 'description' is nullable, so it should be optional
    assert!(
        content.contains("description?: string | null;"),
        "Nullable input field 'description' should be optional. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).expect("std::fs::remove_dir_all failed on temp_dir");
}
