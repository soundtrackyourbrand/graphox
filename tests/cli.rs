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

#[test]
fn test_cli_scoped_fragments() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let output = Command::new(bin_path)
        .arg("--schema")
        .arg("tests/fixtures/simple_schema.graphql")
        .arg("check")
        .arg("tests/fixtures/scoped")
        .output()
        .expect("Failed to execute process");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // pkg_a/query.graphql should be fine because FragmentA is in the same package
    // pkg_b/query.graphql should FAIL because FragmentA is NOT in its package (pkg_b has FragmentB)

    assert!(stdout.contains("pkg_b/query.graphql"));
    assert!(stdout.contains("Unknown fragment: FragmentA"));

    // Optionally check that pkg_a/query.graphql is NOT mentioned as having errors
    // (This depends on how we print, but if it has no errors it won't be printed)
    let pkg_a_output = stdout.contains("pkg_a/query.graphql");
    assert!(!pkg_a_output, "pkg_a/query.graphql should have no errors");
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
