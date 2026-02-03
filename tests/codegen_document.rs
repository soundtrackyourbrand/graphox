use std::process::Command;

#[test]
fn test_codegen_document_node() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_codegen_document_test");
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
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        "const q = gql`query GetUser($id: ID!) { user(id: $id) { id name } }`;",
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
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
    assert!(gen_file.exists(), "Generated file should exist");

    let content = std::fs::read_to_string(gen_file).unwrap();

    // Check for import
    assert!(content.contains("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";"), "Missing DocumentNode import");

    // Check for Document constant export
    assert!(
        content.contains("export const GetUserDocument = {\"definitions\":["),
        "Missing GetUserDocument constant export"
    );
    assert!(
        content.contains("} as unknown as DocumentNode<GetUserQuery, GetUserQueryVariables>;"),
        "Missing correct type cast for DocumentNode. Content:\n{}",
        content
    );

    // Check for variables interface
    assert!(
        content.contains("export interface GetUserQueryVariables {"),
        "Missing Variables interface"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_aliases_and_enums() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_codegen_quirks_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "enum Role { ADMIN USER } type Query { user: User } type User { id: ID! role: Role! }",
    )
    .unwrap();

    // Create a query file with aliases
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        "const q = gql`query GetUser { myUser: user { userId: id userRole: role } }`;",
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
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

    // Check for enums - they are currently inlined in query types
    assert!(
        content.contains("userRole: \"ADMIN\" | \"USER\";"),
        "Missing inlined Role enum. Content:\n{}",
        content
    );

    // Check for aliased fields in Query type
    assert!(
        content.contains("myUser: {"),
        "Missing aliased 'myUser' field. Content:\n{}",
        content
    );
    assert!(
        content.contains("userId: string;"),
        "Missing aliased 'userId' field. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_document_node_no_vars() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_codegen_document_no_vars_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type Query { me: User } type User { id: ID! name: String }",
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        "const q = gql`query GetMe { me { id name } }`;",
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
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
    assert!(gen_file.exists());

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
