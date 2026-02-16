use std::process::Command;

#[test]
#[ntest::timeout(1000)]
fn test_codegen_document_node() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_codegen_document_test");
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
    assert!(gen_file.exists(), "Generated file should exist");

    let content = std::fs::read_to_string(gen_file).unwrap();

    // Check for import
    assert!(content.contains("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";"), "Missing DocumentNode import");

    // Check for Document constant export
    assert!(
        content.contains(
            "export const GetUserQueryDocument = {\"kind\":\"Document\",\"definitions\":["
        ) || content.contains("export const GetUserQueryDocument = {\"definitions\":["),
        "Missing GetUserQueryDocument constant export. Content:\n{}",
        content
    );
    assert!(
        content.contains("} as unknown as DocumentNode<GetUserQuery, GetUserQueryVariables>;"),
        "Missing correct type cast for DocumentNode. Content:\n{}",
        content
    );

    // Check for variables type
    assert!(
        content.contains("export type GetUserQueryVariables = Exact<{"),
        "Missing Variables type"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(1000)]
fn test_codegen_aliases_and_enums() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_codegen_quirks_test");
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
#[ntest::timeout(1000)]
fn test_codegen_document_node_no_vars() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_codegen_document_no_vars_test");
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
    assert!(gen_file.exists());

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(1000)]
fn test_codegen_missing_parent_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_missing_parent_test");
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
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query GetMe { me { id name } }").unwrap();

    // Create YAML config with a nested output_dir that doesn't exist
    let config_file = temp_dir.join("graphox.yaml");
    std::fs::write(
        &config_file,
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "non_existent_parent/generated"
"#,
    )
    .unwrap();

    let output = std::process::Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entrypoint_file = temp_dir.join("non_existent_parent/generated/graphql.ts");
    assert!(
        entrypoint_file.exists(),
        "graphql.ts should exist in nested directory"
    );

    let gen_file = temp_dir.join("non_existent_parent/generated/query.codegen.ts");
    assert!(
        gen_file.exists(),
        "query.codegen.ts should exist in nested directory"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(1000)]
fn test_entrypoint_documents_and_overloads_populated() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_entrypoint_test");
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

    // Create config
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

    // Verify documents export is populated (not empty)
    assert!(
        content.contains(r#"const documents: { [key: string]: any } = {"#),
        "Missing documents export. Content:\n{}",
        content
    );
    assert!(
        content.contains(r#""query GetUser($id: ID!)"#),
        "documents export should contain query source. Content:\n{}",
        content
    );
    assert!(
        content.contains("GetUserQueryDocument"),
        "documents export should reference GetUserQueryDocument. Content:\n{}",
        content
    );

    // Verify specific function overloads are generated
    assert!(
        content.contains(r#"export function graphql(source: "query GetUser($id: ID!)"#),
        "Missing specific graphql overload for GetUser. Content:\n{}",
        content
    );
    assert!(
        content.contains("typeof GetUserQueryDocument"),
        "overload should return typeof GetUserQueryDocument. Content:\n{}",
        content
    );

    // Verify fallback generic overload exists
    assert!(
        content.contains("export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;"),
        "Missing generic fallback overload. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
