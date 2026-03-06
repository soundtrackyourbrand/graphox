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

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(2000)]
fn test_codegen_fragment_ordering() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_codegen_ordering_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type User { id: ID! name: String friends: [User!] } type Query { me: User }",
    )
    .unwrap();

    // Create a query file where fragment usage comes BEFORE fragment definition
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        r#"
        const q = graphql(`
          query GetMe {
            me {
              ...UserFields
            }
          }
        `);

        const f1 = graphql(`
          fragment UserFields on User {
            id
            name
            friends {
              ...FriendFields
            }
          }
        `);

        const f2 = graphql(`
          fragment FriendFields on User {
            id
            name
          }
        `);
        "#,
    )
    .unwrap();

    // Create config - MUST enable generate_ast_for_fragments under codegen
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
    output_dir: "."
    codegen:
      generate_ast_for_fragments: true
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

    // Check ordering: FriendFieldsDocument should be before UserFieldsDocument,
    // and UserFieldsDocument should be before GetMeQueryDocument.

    let friend_doc = "export const FriendFieldsDocument = ";
    let user_doc = "export const UserFieldsDocument = ";
    let query_doc = "export const GetMeQueryDocument = ";

    let friend_pos = content
        .find(friend_doc)
        .unwrap_or_else(|| panic!("FriendFieldsDocument missing in content:\n{}", content));
    let user_pos = content
        .find(user_doc)
        .unwrap_or_else(|| panic!("UserFieldsDocument missing in content:\n{}", content));
    let query_pos = content
        .find(query_doc)
        .unwrap_or_else(|| panic!("GetMeQueryDocument missing in content:\n{}", content));

    assert!(
        friend_pos < user_pos,
        "FriendFieldsDocument should be before UserFieldsDocument"
    );
    assert!(
        user_pos < query_pos,
        "UserFieldsDocument should be before GetMeQueryDocument"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(2000)]
fn test_codegen_fragment_ordering_stable_with_cache_reuse() {
    use ahash::AHashMap;
    use graphox::{
        Config,
        codegen::{CodegenContext, SchemaAnalysisCaches, generate_typescript},
        config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource},
        engine::Engine,
        schema,
    };
    use tower_lsp::lsp_types::PositionEncodingKind;

    let temp_dir = std::env::temp_dir().join("graphox_codegen_cache_ordering_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("query.ts"),
        r#"
        const q = graphql(`
          fragment Parent_Child on User { id }
          fragment Parent on User { ...Parent_Child }
          query GetMe { me { ...Parent } }
        `);
        "#,
    )
    .unwrap();

    let config = Config::new_test(
        temp_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.ts".to_string()))
                .with_codegen(CodegenConfig::enabled().with_generate_ast_for_fragments(true)),
        ],
    );

    let workspace = Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None);
    let project = &config.projects()[0];
    let project_meta = &workspace.projects[0];
    let schema = schema::load_schema(config.base_dir(), project.schema()).unwrap();
    let valid_schema = schema.validate().unwrap();
    let project_context =
        Engine::resolve_project_context(&valid_schema, &workspace.fragments, &project_meta.files)
            .expect("Failed to resolve project context");

    let query_path = project_meta
        .files
        .iter()
        .find(|p| p.ends_with("query.ts"))
        .cloned()
        .expect("project files should include query.ts");
    let doc = workspace
        .documents
        .get(&query_path)
        .expect("query.ts should be parsed during workspace scan");
    let codegen_config = config.get_codegen_config(Some(project));
    assert!(codegen_config.generate_ast_for_fragments());

    let schema_import = None;
    let type_imports: AHashMap<String, String> = AHashMap::default();
    let type_cache = SchemaAnalysisCaches::new();

    let ctx1 = CodegenContext::new(
        &valid_schema,
        &project_context.fragment_to_path,
        &project_context.fragment_to_import,
        &project_context.fragment_to_type_only,
        &project_context.all_fragments,
        &project_context.name_to_id,
        &query_path,
        config.scalars(),
        &schema_import,
        &type_imports,
        codegen_config.generate_ast_for_fragments(),
        &project_context.fragment_dependencies,
        &type_cache,
        &codegen_config,
        "./fragment-masking".to_string(),
        temp_dir.join("query.codegen.ts"),
    );
    let (first_output, _, _) = generate_typescript(doc, &ctx1).unwrap();

    let child_marker = "export const Parent_ChildDocument = ";
    let parent_marker = "export const ParentDocument = ";
    let child_pos = first_output.find(child_marker).unwrap_or_else(|| {
        panic!(
            "Missing child fragment document in first pass:\n{}",
            first_output
        )
    });
    let parent_pos = first_output.find(parent_marker).unwrap_or_else(|| {
        panic!(
            "Missing parent fragment document in first pass:\n{}",
            first_output
        )
    });
    assert!(
        child_pos < parent_pos,
        "Child fragment document should be before parent in first pass"
    );

    let ctx2 = CodegenContext::new(
        &valid_schema,
        &project_context.fragment_to_path,
        &project_context.fragment_to_import,
        &project_context.fragment_to_type_only,
        &project_context.all_fragments,
        &project_context.name_to_id,
        &query_path,
        config.scalars(),
        &schema_import,
        &type_imports,
        codegen_config.generate_ast_for_fragments(),
        &project_context.fragment_dependencies,
        &type_cache,
        &codegen_config,
        "./fragment-masking".to_string(),
        temp_dir.join("query.codegen.ts"),
    );
    let (second_output, _, _) = generate_typescript(doc, &ctx2).unwrap();
    let child_pos_second = second_output.find(child_marker).unwrap_or_else(|| {
        panic!(
            "Missing child fragment document in second pass:\n{}",
            second_output
        )
    });
    let parent_pos_second = second_output.find(parent_marker).unwrap_or_else(|| {
        panic!(
            "Missing parent fragment document in second pass:\n{}",
            second_output
        )
    });
    assert!(
        child_pos_second < parent_pos_second,
        "Child fragment document should remain before parent in cache-backed pass"
    );
    assert_eq!(
        first_output, second_output,
        "Repeated generation with shared cache must remain byte-identical"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(2000)]
fn test_codegen_recursive_fragment_ordering() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_codegen_recursive_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type User { id: ID! friends: [User!] } type Query { me: User }",
    )
    .unwrap();

    // Recursive fragments: A -> B -> A
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        r#"
        const f1 = graphql(`
          fragment FragmentA on User {
            id
            friends {
              ...FragmentB
            }
          }
        `);

        const f2 = graphql(`
          fragment FragmentB on User {
            id
            friends {
              ...FragmentA
            }
          }
        `);

        const q = graphql(`
          query GetMe {
            me {
              ...FragmentA
            }
          }
        `);
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
    codegen:
      generate_ast_for_fragments: true
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

    // In a recursive case, we just want to make sure the Query (which depends on A)
    // is AFTER both A and B.

    let a_doc = "export const FragmentADocument = ";
    let b_doc = "export const FragmentBDocument = ";
    let query_doc = "export const GetMeQueryDocument = ";

    let a_pos = content
        .find(a_doc)
        .unwrap_or_else(|| panic!("FragmentADocument missing in content:\n{}", content));
    let b_pos = content
        .find(b_doc)
        .unwrap_or_else(|| panic!("FragmentBDocument missing in content:\n{}", content));
    let query_pos = content
        .find(query_doc)
        .unwrap_or_else(|| panic!("GetMeQueryDocument missing in content:\n{}", content));

    // Even if they are recursive, the query depends on FragmentA, so FragmentA must be declared first.
    assert!(
        a_pos < query_pos,
        "FragmentADocument should be before GetMeQueryDocument"
    );
    assert!(
        b_pos < query_pos,
        "FragmentBDocument should be before GetMeQueryDocument"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_directives_in_ast() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_repro_directives");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type Query { account(id: ID!): Account location(id: ID!): Location } type Account { id: ID! } type Location { id: ID! }",
    )
    .unwrap();

    // Create a query file with @skip directive
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        "const q = graphql(`
          query GetSessionData($accountId: ID!, $locationId: ID!, $skipLocation: Boolean!) {
            account(id: $accountId) {
              id
            }
            location(id: $locationId) @skip(if: $skipLocation) {
              id
            }
          }
        `);",
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

    // Check if @skip directive is in the AST
    assert!(
        content.contains(r#""kind":"Directive","name":{"kind":"Name","value":"skip"}"#),
        "Missing @skip directive in AST. Content:
{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
