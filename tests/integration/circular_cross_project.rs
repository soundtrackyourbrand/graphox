use std::process::Command;

#[test]
#[ntest::timeout(3000)]
fn test_cli_cross_project_circular_fragments() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_cross_project_circular");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Project 1: public fragment A -> B
    let p1 = temp_dir.join("project1");
    std::fs::create_dir_all(&p1).unwrap();
    std::fs::write(
        p1.join("frag_a.graphql"),
        "fragment FragA on User @public { ...FragB }",
    )
    .unwrap();

    // Project 2: public fragment B -> A
    let p2 = temp_dir.join("project2");
    std::fs::create_dir_all(&p2).unwrap();
    std::fs::write(
        p2.join("frag_b.graphql"),
        "fragment FragB on User @public { ...FragA }",
    )
    .unwrap();

    // Add a query to ensure fragments are considered by projects
    std::fs::write(p2.join("query.graphql"), "query { me { ...FragB } }").unwrap();

    // Create config including both projects
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "project1/**/*.graphql"
  - schema: "schema.graphql"
    include: "project2/**/*.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    // Expect failure due to circular fragment across projects
    assert!(
        !output.status.success(),
        "Check should fail for circular fragments"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Circular fragment reference") || stdout.contains("circular_fragment"),
        "Expected circular fragment diagnostic in output, got: {}",
        stdout
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(3000)]
fn test_cli_cross_project_private_fragments_no_cross() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_cross_project_circular_private");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Project 1: private fragment A -> B
    let p1 = temp_dir.join("proj1");
    std::fs::create_dir_all(&p1).unwrap();
    std::fs::write(
        p1.join("frag_a.graphql"),
        "fragment FragA on User { ...FragB }",
    )
    .unwrap();

    // Project 2: private fragment B -> A
    let p2 = temp_dir.join("proj2");
    std::fs::create_dir_all(&p2).unwrap();
    std::fs::write(
        p2.join("frag_b.graphql"),
        "fragment FragB on User { ...FragA }",
    )
    .unwrap();

    // Add a query in project2 using its private fragment
    std::fs::write(p2.join("query.graphql"), "query { me { ...FragB } }").unwrap();

    // Create config including both projects
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "proj1/**/*.graphql"
  - schema: "schema.graphql"
    include: "proj2/**/*.graphql"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .current_dir(&temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    // Expect success because fragments are private and should not be visible across projects
    assert!(
        output.status.success(),
        "Check should succeed when fragments are private"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No issues found."),
        "Expected no issues, got: {}",
        stdout
    );

    std::fs::remove_dir_all(temp_dir).ok();
}
