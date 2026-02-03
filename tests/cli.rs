use std::path::Path;
use std::process::Command;

#[test]
fn test_cli_check_no_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let output = Command::new(bin_path)
        .arg("--schema")
        .arg("tests/fixtures/simple_schema.graphql")
        .arg("check")
        .arg("tests/fixtures/component.ts") // Single file that has no deprecations
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No issues found."));
}

#[test]
fn test_cli_check_with_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let output = Command::new(bin_path)
        .arg("--schema")
        .arg("tests/fixtures/simple_schema.graphql")
        .arg("check")
        .arg("tests/fixtures/deprecated.graphql") // This file has one deprecation
        .output()
        .expect("Failed to execute process");

    // It should exit with 1 because it found deprecations
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Field 'oldField' is deprecated"));
    assert!(stdout.contains("Use username instead"));
}

#[test]
fn test_cli_ignore_files() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_ignore_test");
    std::fs::create_dir_all(&temp_dir).ok();

    let query_file = temp_dir.join("should_be_ignored.graphql");
    // This file has an error (unknown field)
    std::fs::write(&query_file, "query { users { nonExistentField } }").unwrap();

    let ignore_file = temp_dir.join(".graphqlignore");
    std::fs::write(&ignore_file, "should_be_ignored.graphql").unwrap();

    let output = Command::new(bin_path)
        .arg("--schema")
        .arg("tests/fixtures/simple_schema.graphql")
        .arg("check")
        .arg(temp_dir.to_str().unwrap())
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
fn test_cli_ignore_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_ignore_deprecations_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
type Query {
  oldField: String @deprecated(reason: "Use newField instead")
  ignoredField: String @deprecated(reason: "Internal use only")
}
"#,
    )
    .unwrap();

    // Create config
    let config_file = temp_dir.join("graphql.yaml");
    std::fs::write(
        &config_file,
        r#"
ignore_deprecations:
  - "Internal.*"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
"#,
    )
    .unwrap();

    // Create query
    let query_file = temp_dir.join("query.graphql");
    std::fs::write(&query_file, "query { oldField ignoredField }").unwrap();

    let output = Command::new(bin_path)
        .arg("check")
        .arg(".")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should FAIL because oldField is NOT ignored
    assert!(
        !output.status.success(),
        "Check should fail due to non-ignored deprecation"
    );

    // Should contain oldField warning
    assert!(
        stdout.contains("Field 'oldField' is deprecated"),
        "Missing warning for oldField"
    );

    // Should NOT contain ignoredField warning
    assert!(
        !stdout.contains("Field 'ignoredField' is deprecated"),
        "Should not warn for ignoredField"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_cli_public_fragments() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let output = Command::new(bin_path)
        .arg("--schema")
        .arg("tests/fixtures/simple_schema.graphql")
        .arg("check")
        .arg("tests/fixtures/public_test")
        .output()
        .expect("Failed to execute process");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // pkg_b/query.graphql:
    // ...PublicFrag should be allowed
    // ...PrivateFrag should NOT be allowed

    assert!(stdout.contains("Unknown fragment: PrivateFrag"));
    assert!(
        !stdout.contains("Unknown fragment: PublicFrag"),
        "PublicFrag should be visible across packages"
    );
}

#[test]
fn test_cli_config_file() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_config_test");
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
fn test_cli_config_output_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_config_output_test");
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
fn test_cli_check_input_deprecations() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_input_deprecations_test");
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

    let output = Command::new(bin_path)
        .arg("--schema")
        .arg(schema_file.to_str().unwrap())
        .arg("check")
        .arg(query_file.to_str().unwrap())
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
fn test_cli_schema_types() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_schema_types_test");
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
fn test_cli_custom_scalars() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let temp_dir = std::env::temp_dir().join("graphql_rust_scalars_test");
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
fn test_cli_codegen_baselines() {
    run_baseline_test(
        "tests/fixtures/codegen",
        "tests/baselines/codegen",
        Some("tests/fixtures/simple_schema.graphql"),
    );
}

#[test]
fn test_cli_schema_import_baselines() {
    run_baseline_test(
        "tests/fixtures/schema_import",
        "tests/baselines/schema_import",
        None,
    );
}

#[test]
fn test_cli_project_import_baselines() {
    run_baseline_test(
        "tests/fixtures/project_import",
        "tests/baselines/project_import",
        None,
    );
}

fn run_baseline_test(fixture_dir_str: &str, baseline_dir_str: &str, schema_path: Option<&str>) {
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
    if let Some(s) = schema_path {
        let abs_schema = std::fs::canonicalize(s).expect("Failed to canonicalize schema path");
        cmd.arg("--schema").arg(abs_schema);
    }

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

    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("graphql") {
            let file_stem = path.file_stem().unwrap().to_str().unwrap();

            let mut codegen_path = temp_dir.clone();
            codegen_path.push(format!("{}.codegen.ts", file_stem));

            let expected_path = baseline_dir.join(format!("{}.expected.ts", file_stem));

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
                println!("--- ACTUAL ---");
                println!("{}", actual);
                println!("--- EXPECTED ---");
                println!("{}", expected);
                panic!("Codegen mismatch for {:?} in {}", path, fixture_dir_str);
            }
        }
    }

    std::fs::remove_dir_all(temp_dir).ok();
}
