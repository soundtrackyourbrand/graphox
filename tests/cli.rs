use std::path::Path;
use std::process::Command;

#[test]
#[ntest::timeout(100)]
fn test_cli_check_no_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_check_no_deprecations");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Copy schema and file
    std::fs::copy(
        "tests/fixtures/simple_schema.graphql",
        temp_dir.join("schema.graphql"),
    )
    .unwrap();
    std::fs::copy("tests/fixtures/component.ts", temp_dir.join("component.ts")).unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "component.ts"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No issues found."));

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_check_with_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_check_with_deprecations");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Copy schema and file
    std::fs::copy(
        "tests/fixtures/simple_schema.graphql",
        temp_dir.join("schema.graphql"),
    )
    .unwrap();
    std::fs::copy(
        "tests/fixtures/deprecated.graphql",
        temp_dir.join("deprecated.graphql"),
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "deprecated.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    // It should exit with 1 because it found deprecations
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Field 'oldField' is deprecated"));
    assert!(stdout.contains("Use username instead"));

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_ignore_files() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_ignore_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    std::fs::copy(
        "tests/fixtures/simple_schema.graphql",
        temp_dir.join("schema.graphql"),
    )
    .unwrap();

    let query_file = temp_dir.join("should_be_ignored.graphql");
    // This file has an error (unknown field)
    std::fs::write(&query_file, "query { users { nonExistentField } }").unwrap();

    let ignore_file = temp_dir.join(".graphqlignore");
    std::fs::write(&ignore_file, "should_be_ignored.graphql").unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "**/*.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    // Should succeed because the buggy file is ignored
    assert!(
        output.status.success(),
        "Check failed even though file should be ignored: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("No issues found."));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_codegen_error() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_codegen_error_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Create buggy query (unknown field)
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query { me { unknownField } }").unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    // It should exit with 1 because codegen failed for the buggy query
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknownField"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_codegen_invalid_schema() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_codegen_invalid_schema_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create invalid schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(&schema_file, "type Query { me: NonExistentType }").unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "*.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    // It should exit with 1 because schema validation failed
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("NonExistentType"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_codegen_clean() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_codegen_clean_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Create query
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query { me { id name } }").unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
"#,
    )
    .unwrap();

    // 1. Run codegen
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let gen_file = temp_dir.join("query.codegen.ts");
    assert!(gen_file.exists());

    // 2. Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .arg("--verbose")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen clean failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !gen_file.exists(),
        "Generated file should have been removed"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Removed"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_check_verbose_ignored_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_check_verbose_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema with deprecation
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
type Query {
  oldField: String @deprecated(reason: "Use newField instead")
  newField: String
}
"#,
    )
    .unwrap();

    // Create config that ignores it
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        r#"
ignore_deprecations:
  - "Use newField.*"
projects:
  - schema: "schema.graphql"
    include: "**/*.graphql"
"#,
    )
    .unwrap();

    // Create query using deprecated field
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query { oldField }").unwrap();

    // 1. Run check normally (should succeed with no output)
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Check failed. STDOUT: {}, STDERR: {}",
        stdout,
        stderr
    );
    assert!(stdout.contains("No issues found."));

    // 2. Run check --verbose (should succeed but show ignored deprecation)
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .arg("--verbose")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Check verbose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[Ignored] Field 'oldField' is deprecated"));
    assert!(stdout.contains("Info"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_fragment_ast_generation() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_fragment_ast_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Create fragment file
    let fragment_file = temp_dir.join("fragment.graphql");
    std::fs::write(
        &fragment_file,
        "fragment UserFields on User @public { id name }",
    )
    .unwrap();

    // Create operation file
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query GetMe { me { ...UserFields } }").unwrap();

    // Create config
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        r#"
generate_ast_for_fragments: true
projects:
  - schema: "schema.graphql"
    include: "**/*.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify fragment codegen
    let frag_gen = temp_dir.join("fragment.codegen.ts");
    assert!(frag_gen.exists());
    let frag_content = std::fs::read_to_string(&frag_gen).unwrap();
    assert!(frag_content.contains("export const UserFieldsDocument"));
    assert!(frag_content.contains("\"kind\":\"FragmentDefinition\""));

    // Verify query codegen
    let query_gen = temp_dir.join("query.codegen.ts");
    assert!(query_gen.exists());
    let query_content = std::fs::read_to_string(&query_gen).unwrap();

    // Check for import of fragment document
    assert!(query_content.contains("import { UserFieldsDocument } from \"./fragment.codegen\";"));

    // Check that it uses the fragment document definitions
    assert!(query_content.contains("...UserFieldsDocument.definitions"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_graphql_entrypoint() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_entrypoint_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.graphql");
    let query_text = "query GetMe { me { id name } }";
    std::fs::write(&query_file, query_text).unwrap();

    // Create YAML config
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        r#"
output_dir: "gen"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
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

    let entrypoint_file = temp_dir.join("gen").join("graphql.ts");
    assert!(
        entrypoint_file.exists(),
        "graphql.ts entrypoint was not created"
    );

    let content = std::fs::read_to_string(entrypoint_file).unwrap();
    println!("--- ENTRYPOINT CONTENT ---\n{}", content);

    // Check for imports

    assert!(content.contains(
        "import { GetMeQuery, GetMeQueryVariables, GetMeQueryDocument } from \"./query.codegen\";"
    ));

    // Check for graphql function overloads
    assert!(content.contains("export function graphql(source: \"query GetMe { me { id name } }\"): typeof GetMeQueryDocument;"));

    // Check for gql export
    assert!(content.contains("export const gql = graphql;"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_config_file() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_config_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query { user { id } }").unwrap();

    // Create YAML config
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        format!(
            r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
"#
        ),
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("check")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Check failed with config file: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("No issues found."));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_config_output_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_config_output_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query GetUser { user { id } }").unwrap();

    // Create YAML config
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        r#"
output_dir: "gen"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
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
        "Codegen failed with config file: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected_gen_file = temp_dir.join("gen").join("query.codegen.ts");
    assert!(
        expected_gen_file.exists(),
        "Generated file {:?} does not exist. Output: {}",
        expected_gen_file,
        String::from_utf8_lossy(&output.stdout)
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_check_input_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_input_deprecations_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema with Deprecated Input and Deprecated Input Field
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
input OldInput @deprecated(reason: "Use NewInput") {
  field: String
}

input NewInput {
  oldField: String @deprecated(reason: "Use newField")
  newField: String
}

type Query {
  test(old: OldInput, new: NewInput): String
}
"#,
    )
    .unwrap();

    // Create a query file using deprecated things
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(
        &query_file,
        r#"
query Test($old: OldInput) {
  test(old: $old, new: { oldField: "value" })
}
"#,
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphql.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("Check Output:\n{}", stdout);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("Type 'OldInput' is deprecated: Use NewInput"));
    assert!(stdout.contains("Input field 'oldField' is deprecated: Use newField"));

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_schema_types() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_schema_types_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    let schema_fixture = "tests/fixtures/schema_types/schema.graphql";
    let baseline_file = "tests/baselines/schema_types/schema_types.expected.ts";
    let gen_output_path = temp_dir.join("schema_types.ts");

    // Create YAML config
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        format!(
            r#"
projects: []
schema_types:
  - schema: "{}"
    output: "{}"
"#,
            std::fs::canonicalize(schema_fixture).unwrap().display(),
            gen_output_path.display()
        ),
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed for schema types: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        gen_output_path.exists(),
        "Schema types file was not created"
    );

    let actual = std::fs::read_to_string(gen_output_path).unwrap();
    let expected = std::fs::read_to_string(baseline_file).unwrap();

    if actual.trim() != expected.trim() {
        println!("--- ACTUAL ---");
        println!("{}", actual);
        println!("--- EXPECTED ---");
        println!("{}", expected);
        panic!("Schema types mismatch");
    }

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_custom_scalars() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_scalars_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema with custom scalar
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
scalar DateTime

type Query {
  now: DateTime!
}
"#,
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(
        &query_file,
        r#"
query GetNow {
  now
}
"#,
    )
    .unwrap();

    // Create YAML config with scalar mapping
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
schema_types:
  - schema: "schema.graphql"
    output: "schema_types.ts"
scalars:
  DateTime: "Date"
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
        "Codegen failed with custom scalars: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let gen_file = temp_dir.join("query.codegen.ts");
    assert!(gen_file.exists(), "Codegen file was not created");

    let content = std::fs::read_to_string(gen_file).unwrap();
    assert!(
        content.contains("now: Date;"),
        "Custom scalar DateTime was not mapped to Date in operation types. Content:\n{}",
        content
    );

    let schema_types_file = temp_dir.join("schema_types.ts");
    assert!(
        schema_types_file.exists(),
        "Schema types file was not created"
    );
    let schema_content = std::fs::read_to_string(schema_types_file).unwrap();
    assert!(
        schema_content.contains("export type DateTime = Date;"),
        "Custom scalar DateTime was not mapped to Date in schema types. Content:\n{}",
        schema_content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(100)]
fn test_cli_codegen_baselines() {
    run_baseline_test("tests/fixtures/codegen", "tests/baselines/codegen", None);
}

#[test]
#[ntest::timeout(100)]
fn test_cli_schema_import_baselines() {
    run_baseline_test(
        "tests/fixtures/schema_import",
        "tests/baselines/schema_import",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_project_import_baselines() {
    run_baseline_test(
        "tests/fixtures/project_import",
        "tests/baselines/project_import",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_multi_schema_import_baselines() {
    run_baseline_test(
        "tests/fixtures/multi_schema_import",
        "tests/baselines/multi_schema_import",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_multi_schema_import_superset_baselines() {
    run_baseline_test(
        "tests/fixtures/multi_schema_import_superset",
        "tests/baselines/multi_schema_import_superset",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_public_test_baselines() {
    run_baseline_test(
        "tests/fixtures/public_test",
        "tests/baselines/public_test",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_fragment_ast_baselines() {
    run_baseline_test(
        "tests/fixtures/fragment_ast",
        "tests/baselines/fragment_ast",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_entrypoint_baselines() {
    run_baseline_test(
        "tests/fixtures/entrypoint",
        "tests/baselines/entrypoint",
        None,
    );
}

#[test]
#[ntest::timeout(100)]
fn test_cli_aliases_baselines() {
    run_baseline_test("tests/fixtures/aliases", "tests/baselines/aliases", None);
}

fn run_baseline_test(fixture_dir_str: &str, baseline_dir_str: &str, _schema_path: Option<&str>) {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let fixture_dir = Path::new(fixture_dir_str);
    let baseline_dir = Path::new(baseline_dir_str);
    let temp_dir = std::env::temp_dir().join(format!(
        "graphql_rust_baselines_{}",
        fixture_dir_str.replace("/", "_")
    ));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    let mut cmd = Command::new(bin_path);

    let output = cmd
        .arg("codegen")
        .arg(".")
        .arg("--output")
        .arg(temp_dir.to_str().unwrap())
        .current_dir(fixture_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen command failed for {}: {}",
        fixture_dir_str,
        String::from_utf8_lossy(&output.stderr)
    );

    let mut stack = vec![fixture_dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().unwrap() == "gen" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) == Some("graphql") {
                let rel_to_fixture = path.strip_prefix(fixture_dir).unwrap();
                let file_stem = rel_to_fixture.file_stem().unwrap().to_str().unwrap();
                let parent = rel_to_fixture.parent().unwrap();

                let mut codegen_path = temp_dir.clone();
                codegen_path.push(rel_to_fixture);
                codegen_path.set_extension("codegen.ts");

                let expected_path = baseline_dir
                    .join(parent)
                    .join(format!("{}.expected.ts", file_stem));

                if !expected_path.exists() {
                    continue;
                }

                assert!(
                    codegen_path.exists(),
                    "Codegen file {:?} was not created",
                    codegen_path
                );

                let actual = std::fs::read_to_string(&codegen_path).unwrap();
                let expected = std::fs::read_to_string(&expected_path).unwrap();

                if actual.trim() != expected.trim() {
                    println!("--- ACTUAL ({:?}) ---", path);
                    println!("{}", actual);
                    println!("--- EXPECTED ---");
                    println!("{}", expected);
                    panic!("Codegen mismatch for {:?} in {}", path, fixture_dir_str);
                }
            }
        }
    }

    std::fs::remove_dir_all(temp_dir).ok();
}
