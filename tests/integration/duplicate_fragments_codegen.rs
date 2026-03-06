use std::fs;
use std::process::Command;

#[test]
fn test_duplicate_fragments_codegen_resolution() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("tmp")
        .join("duplicate_fragments_codegen");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).ok();
    }
    fs::create_dir_all(temp_dir.join("pkg_a")).unwrap();
    fs::create_dir_all(temp_dir.join("pkg_b")).unwrap();
    let temp_dir = fs::canonicalize(&temp_dir).unwrap();

    // 1. Schema
    fs::write(
        temp_dir.join("schema.graphql"),
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    // 2. Project A fragment and query
    fs::write(
        temp_dir.join("pkg_a/fragments.graphql"),
        "fragment CommonFrag on User @public { id }",
    )
    .unwrap();
    fs::write(
        temp_dir.join("pkg_a/query.graphql"),
        "query QueryA { user { ...CommonFrag } }",
    )
    .unwrap();

    // 3. Project B fragment with same name
    fs::write(
        temp_dir.join("pkg_b/fragments.graphql"),
        "fragment CommonFrag on User @public { name }",
    )
    .unwrap();

    // 4. Config
    fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "pkg_a/**/*.graphql"
    output_dir: "pkg_a/__generated__"
  - schema: "schema.graphql"
    include: "pkg_b/**/*.graphql"
    output_dir: "pkg_b/__generated__"
"#,
    )
    .unwrap();

    // 5. Run codegen
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

    // 6. Check pkg_a codegen output
    let codegen_file = temp_dir.join("pkg_a/__generated__/query.codegen.ts");
    assert!(
        codegen_file.exists(),
        "Codegen file not found at {:?}",
        codegen_file
    );

    let content = fs::read_to_string(codegen_file).unwrap();

    // It should import CommonFrag from its own directory, NOT from pkg_b
    assert!(
        content.contains("./fragments.codegen"),
        "Should import from local ./fragments.codegen, but got:\n{}",
        content
    );
    assert!(
        !content.contains("pkg_b"),
        "Should NOT import from pkg_b project"
    );

    // Cleanup
    fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_cross_project_fragment_import_when_no_local_exists() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("tmp")
        .join("cross_project_import");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).ok();
    }
    fs::create_dir_all(temp_dir.join("pkg_a")).unwrap();
    fs::create_dir_all(temp_dir.join("pkg_b")).unwrap();
    let temp_dir = fs::canonicalize(&temp_dir).unwrap();

    // 1. Schema
    fs::write(
        temp_dir.join("schema.graphql"),
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    // 2. Project A query using Project B fragment
    fs::write(
        temp_dir.join("pkg_a/query.graphql"),
        "query QueryA { user { ...FragB } }",
    )
    .unwrap();

    // 3. Project B fragment
    fs::write(
        temp_dir.join("pkg_b/fragments.graphql"),
        "fragment FragB on User @public { name }",
    )
    .unwrap();

    // 4. Config
    fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "pkg_a/**/*.graphql"
    output_dir: "pkg_a/__generated__"
  - schema: "schema.graphql"
    include: "pkg_b/**/*.graphql"
    output_dir: "pkg_b/__generated__"
"#,
    )
    .unwrap();

    // 5. Run codegen
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

    // 6. Check pkg_a codegen output
    let codegen_file = temp_dir.join("pkg_a/__generated__/query.codegen.ts");
    assert!(codegen_file.exists());

    let content = fs::read_to_string(codegen_file).unwrap();

    // It should import FragB from pkg_b
    assert!(
        content.contains("pkg_b/fragments.codegen"),
        "Should import from pkg_b project, but got:\n{}",
        content
    );

    // Cleanup
    fs::remove_dir_all(temp_dir).ok();
}
