use std::process::Command;

#[test]
fn test_codegen_clean_with_output_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_clean_output_dir_test");
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

    // Create query in src
    let src_dir = temp_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let query_file = src_dir.join("query.graphql");
    std::fs::write(&query_file, "query { me { id name } }").unwrap();

    // Create config with output_dir
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "src/**/*.graphql"
    output_dir: "generated"
"#,
    )
    .unwrap();

    // 1. Run codegen
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());

    let gen_dir = temp_dir.join("generated");
    assert!(gen_dir.exists());
    assert!(gen_dir.join("query.codegen.ts").exists());
    assert!(gen_dir.join("graphql.ts").exists());

    // 2. Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    assert!(!gen_dir.exists(), "Output directory should be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_clean_multiple_includes() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_clean_multiple_includes_test");
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

    // Create queries in different dirs
    let src1 = temp_dir.join("src1");
    std::fs::create_dir_all(&src1).unwrap();
    std::fs::write(src1.join("q1.graphql"), "query { me { id } }").unwrap();

    let src2 = temp_dir.join("src2");
    std::fs::create_dir_all(&src2).unwrap();
    std::fs::write(src2.join("q2.graphql"), "query { me { name } }").unwrap();

    // Create config with multiple includes and NO output_dir
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include:
      - "src1/**/*.graphql"
      - "src2/**/*.graphql"
"#,
    )
    .unwrap();

    // 1. Run codegen
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());

    let gen1 = temp_dir.join("q1.codegen.ts");
    let gen2 = temp_dir.join("q2.codegen.ts");
    assert!(gen1.exists(), "gen1 should exist");
    assert!(gen2.exists(), "gen2 should exist");

    // 2. Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    assert!(!gen1.exists(), "gen1 should be removed");
    assert!(!gen2.exists(), "gen2 should be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_surgical_clean() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_surgical_clean_test");
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
    std::fs::write(&query_file, "query { me { id } }").unwrap();

    // Create a manual file that should NOT be removed
    let manual_file = temp_dir.join("important.ts");
    std::fs::write(&manual_file, "const x = 1;").unwrap();

    // Create config with output_dir set to "."
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

    // 1. Run codegen
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());

    let gen_file = temp_dir.join("query.codegen.ts");
    assert!(gen_file.exists());
    assert!(temp_dir.join("graphql.ts").exists());

    // 2. Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    assert!(!gen_file.exists(), "query.codegen.ts should be removed");
    assert!(
        !temp_dir.join("graphql.ts").exists(),
        "graphql.ts should be removed"
    );
    assert!(manual_file.exists(), "important.ts should NOT be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_clean_default_generated_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_clean_default_gen_test");
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
    std::fs::write(&query_file, "query { me { id } }").unwrap();

    // Create config WITHOUT output_dir
    std::fs::write(
        temp_dir.join("graphox.yaml"),
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

    assert!(output.status.success());

    assert!(temp_dir.join("query.codegen.ts").exists());
    let default_gen_dir = temp_dir.join("__generated__");
    assert!(
        default_gen_dir.exists(),
        "__generated__ should be created by default"
    );
    assert!(default_gen_dir.join("graphql.ts").exists());

    // 2. Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    assert!(
        !temp_dir.join("query.codegen.ts").exists(),
        "query.codegen.ts should be removed"
    );
    assert!(!default_gen_dir.exists(), "__generated__ should be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_surgical_clean_recursive() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_surgical_clean_recursive_test");
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

    // Create query in src
    let src_dir = temp_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let query_file = src_dir.join("query.graphql");
    std::fs::write(&query_file, "query { me { id } }").unwrap();

    // Create query in src/features/user
    let deeper_dir = src_dir.join("features/user");
    std::fs::create_dir_all(&deeper_dir).unwrap();
    let deeper_query = deeper_dir.join("user.graphql");
    std::fs::write(&deeper_query, "query { me { name } }").unwrap();

    // Create config with output_dir set to "."
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "src/**/*.graphql"
    output_dir: "."
"#,
    )
    .unwrap();

    // 1. Run codegen
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    if !output.status.success() {
        panic!(
            "Codegen failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // With output_dir="." and include="src/**/*.graphql", include_prefix is "src"
    // So src/query.graphql -> query.codegen.ts
    // src/features/user/user.graphql -> features/user/user.codegen.ts
    let gen_file1 = temp_dir.join("query.codegen.ts");
    let gen_file2 = temp_dir.join("features/user/user.codegen.ts");
    assert!(gen_file1.exists());
    assert!(gen_file2.exists());

    // 2. Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    if !output.status.success() {
        panic!(
            "Codegen clean failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!gen_file1.exists(), "query.codegen.ts should be removed");
    assert!(!gen_file2.exists(), "user.codegen.ts should be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_clean_disabled_project() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_clean_disabled_project_test");
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
    std::fs::write(&query_file, "query { me { id } }").unwrap();

    // 1. Create config with codegen ENABLED
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "generated"
"#,
    )
    .unwrap();

    // Run codegen to create files
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");
    assert!(output.status.success());
    assert!(temp_dir.join("generated/query.codegen.ts").exists());

    // 2. Update config to DISABLE codegen
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.graphql"
    output_dir: "generated"
    codegen: false
"#,
    )
    .unwrap();

    // Run codegen --clean
    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    // Currently this MIGHT fail because disabled projects are filtered out
    assert!(
        !temp_dir.join("generated").exists(),
        "generated directory should be removed even if project is disabled"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
