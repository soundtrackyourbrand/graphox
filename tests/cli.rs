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

#[test]
fn test_cli_codegen_baselines() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
    let fixture_dir = Path::new("tests/fixtures/codegen");
    let baseline_dir = Path::new("tests/baselines/codegen");
    let temp_dir = std::env::temp_dir().join("graphql_rust_baselines_test");
    std::fs::create_dir_all(&temp_dir).ok();

    // We run codegen on the whole fixture directory, outputting to a temp directory
    let output = Command::new(bin_path)
        .arg("--schema")
        .arg("tests/fixtures/simple_schema.graphql")
        .arg("codegen")
        .arg(fixture_dir.to_str().unwrap())
        .arg("--output")
        .arg(temp_dir.to_str().unwrap())
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // For each .graphql file in fixtures, check if there is an .expected.ts file in baselines and compare
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("graphql") {
            let file_stem = path.file_stem().unwrap().to_str().unwrap();

            // Generated file is in the temp directory
            let mut codegen_path = temp_dir.clone();
            codegen_path.push(path.strip_prefix(fixture_dir).unwrap());
            codegen_path.set_extension("graphql.codegen.ts");

            // Expected file is in the baseline directory
            let expected_path = baseline_dir.join(format!("{}.expected.ts", file_stem));

            assert!(
                codegen_path.exists(),
                "Codegen file {:?} was not created",
                codegen_path
            );
            assert!(
                expected_path.exists(),
                "Expected file {:?} does not exist",
                expected_path
            );

            let actual = std::fs::read_to_string(&codegen_path).unwrap();
            let expected = std::fs::read_to_string(&expected_path).unwrap();

            if actual.trim() != expected.trim() {
                println!("--- ACTUAL ---");
                println!("{}", actual);
                println!("--- EXPECTED ---");
                println!("{}", expected);
                panic!("Codegen mismatch for {:?}", path);
            }
        }
    }

    // Cleanup temp directory
    std::fs::remove_dir_all(temp_dir).ok();
}
