use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_codegen_import_input_types() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let dir = tempdir().unwrap();
    let temp_dir = dir.path();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    fs::write(
        &schema_file,
        r#"
        enum Role { ADMIN USER }
        input UserInput { id: ID!, role: Role! }
        type Query { user(input: UserInput): User }
        type User { id: ID! name: String }
        "#,
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.ts");
    fs::write(
        &query_file,
        "const q = gql`query GetUser($input: UserInput) { user(input: $input) { id name } }`;",
    )
    .unwrap();

    // Create config
    fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
schema_types:
  - schema: "schema.graphql"
    output: "types.ts"
    import: "@/types"
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
    let content = fs::read_to_string(gen_file).unwrap();
    println!("Generated content:\n{}", content);

    // It should import UserInput
    // We don't necessarily expect it to import Role unless it's used in the operation output or variables directly.
    // In this case, Role is used inside UserInput, but UserInput itself is what's used as a variable.
    assert!(
        content.contains("import type { UserInput } from \"@/types\";"),
        "Should import UserInput from '@/types'. Content:\n{}",
        content
    );
}
