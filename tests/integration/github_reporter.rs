use std::process::Command;

#[test]
fn test_cli_check_github_reporter() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_github_reporter_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Copy schema and file with deprecation
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
        temp_dir.join("graphox.yaml"),
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
        .arg("--reporter")
        .arg("github")
        .output()
        .expect("Failed to execute process");

    // It should exit with 1 because it found issues
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Check for GitHub annotation format
    // ::warning file=deprecated.graphql,line=4,col=9::Field 'oldField' is deprecated: Use username instead
    if !stdout.contains("::warning file=")
        || !stdout.contains(
            "deprecated.graphql,line=4,col=9::Field 'oldField' is deprecated: Use username instead",
        )
    {
        panic!(
            "STDOUT did not contain expected GitHub annotation.\nSTDOUT:\n{}\nSTDERR:\n{}",
            stdout, stderr
        );
    }

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_cli_check_github_reporter_duplicates() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_github_reporter_duplicates");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Copy schema
    std::fs::copy(
        "tests/fixtures/simple_schema.graphql",
        temp_dir.join("schema.graphql"),
    )
    .unwrap();

    // Create two files with same operation name
    std::fs::write(
        temp_dir.join("op1.graphql"),
        "query GetUser { users { id } }",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("op2.graphql"),
        "query GetUser { users { username } }",
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "*.graphql"
rules:
  unique_operation_name: true
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .arg("--reporter")
        .arg("github")
        .output()
        .expect("Failed to execute process");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for duplicate operation annotations
    assert!(stdout.contains(
        "::error file=op1.graphql::Duplicate operation name 'GetUser' in project *.graphql"
    ));
    assert!(stdout.contains(
        "::error file=op2.graphql::Duplicate operation name 'GetUser' in project *.graphql"
    ));

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_cli_check_tsc_reporter() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_tsc_reporter_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Copy schema and file with deprecation
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
        temp_dir.join("graphox.yaml"),
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
        .arg("--reporter")
        .arg("tsc")
        .output()
        .expect("Failed to execute process");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for tsc format: path(line,col): severity: message
    assert!(stdout.contains(
        "deprecated.graphql(4,9): warning: Field 'oldField' is deprecated: Use username instead"
    ));

    std::fs::remove_dir_all(temp_dir).ok();
}
