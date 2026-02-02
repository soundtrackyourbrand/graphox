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
    assert!(stdout.contains("No deprecation warnings found."));
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
fn test_cli_help() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let output = Command::new(bin_path)
        .arg("--help")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("lsp"));
    assert!(stdout.contains("check"));
}
