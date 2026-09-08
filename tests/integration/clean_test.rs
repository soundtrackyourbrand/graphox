use crate::support::cmd;
use crate::support::cmd::{assert_command_succeeded, fresh_dir, graphox};

#[test]
fn test_codegen_clean_with_output_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_clean_output_dir_test");

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
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen", &temp_dir);

    let gen_dir = temp_dir.join("generated");
    assert!(gen_dir.exists());
    assert!(gen_dir.join("query.codegen.ts").exists());
    assert!(gen_dir.join("graphql.ts").exists());

    // 2. Run codegen --clean
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen --clean", &temp_dir);
    assert!(!gen_dir.exists(), "Output directory should be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_clean_multiple_includes() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_clean_multiple_includes_test");

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
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen", &temp_dir);

    let gen1 = temp_dir.join("q1.codegen.ts");
    let gen2 = temp_dir.join("q2.codegen.ts");
    assert!(gen1.exists(), "gen1 should exist");
    assert!(gen2.exists(), "gen2 should exist");

    // 2. Run codegen --clean
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen --clean", &temp_dir);
    assert!(!gen1.exists(), "gen1 should be removed");
    assert!(!gen2.exists(), "gen2 should be removed");

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_surgical_clean() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_surgical_clean_test");

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
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen", &temp_dir);

    let gen_file = temp_dir.join("query.codegen.ts");
    assert!(gen_file.exists());
    assert!(temp_dir.join("graphql.ts").exists());

    // 2. Run codegen --clean
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen --clean", &temp_dir);
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
    let temp_dir = fresh_dir("graphox_clean_default_gen_test");

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
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen", &temp_dir);

    assert!(temp_dir.join("query.codegen.ts").exists());
    let default_gen_dir = temp_dir.join("__generated__");
    assert!(
        default_gen_dir.exists(),
        "__generated__ should be created by default"
    );
    assert!(default_gen_dir.join("graphql.ts").exists());

    // 2. Run codegen --clean
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen --clean", &temp_dir);
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
    let temp_dir = fresh_dir("graphox_surgical_clean_recursive_test");

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
    let output = graphox(bin_path, &temp_dir)
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
    let output = graphox(bin_path, &temp_dir)
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
    let temp_dir = fresh_dir("graphox_clean_disabled_project_test");

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
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");
    assert_command_succeeded(&output, "codegen", &temp_dir);
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
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen --clean", &temp_dir);
    // Currently this MIGHT fail because disabled projects are filtered out
    assert!(
        !temp_dir.join("generated").exists(),
        "generated directory should be removed even if project is disabled"
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_codegen_clean_with_missing_schema_still_removes_output_dir() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_clean_missing_schema_test");

    std::fs::write(temp_dir.join("query.graphql"), "query { me { id } }").unwrap();
    let generated_dir = temp_dir.join("generated");
    std::fs::create_dir_all(&generated_dir).unwrap();
    std::fs::write(generated_dir.join("query.codegen.ts"), "// generated").unwrap();

    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "missing-schema.graphql"
    include: "query.graphql"
    output_dir: "generated"
"#,
    )
    .unwrap();

    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "clean should not depend on loading schemas: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !generated_dir.exists(),
        "generated directory should be removed even when schema is missing"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_check_skips_output_dir_inside_include_root() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_skip_output_dir_scan_test");

    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();

    let src_dir = temp_dir.join("src");
    let generated_dir = src_dir.join("graphql");
    std::fs::create_dir_all(&generated_dir).unwrap();
    std::fs::write(src_dir.join("query.graphql"), "query { me { id } }").unwrap();
    std::fs::write(
        generated_dir.join("broken.graphql"),
        "query BrokenOutput { missing }",
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "src"
    output_dir: "src/graphql"
"#,
    )
    .unwrap();

    let output = graphox(bin_path, &temp_dir)
        .arg("check")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "output_dir should be excluded from scan: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

/// `--clean` promises the generated output is gone. The schema cache is derived
/// data behind that promise, and it is shared with every other graphox process
/// on the machine, so a clean that cannot empty it has still done its job.
///
/// Holding a cache file open is what provokes this on Windows: the file stays in
/// the directory as delete-pending until the handle closes, and removing the
/// parent then fails with "Access is denied". Unix removes an open file happily,
/// so there the test only pins the exit status and the output removal.
#[test]
fn test_codegen_clean_succeeds_while_the_cache_is_held_open() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_clean_cache_held_open_test");

    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();
    std::fs::write(temp_dir.join("query.graphql"), "query { me { id } }").unwrap();
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

    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");
    assert_command_succeeded(&output, "codegen", &temp_dir);

    let generated_dir = temp_dir.join("generated");
    assert!(generated_dir.exists(), "codegen should have written output");

    // Hold every cache entry open across the clean.
    //
    // Plain `File::open` is deliberate. Windows grants FILE_SHARE_DELETE by
    // default, which is what makes the delete *succeed* and leave the entry
    // behind as delete-pending — the state that fails the parent's removal with
    // "Access is denied", and the one the CI failure this fixes reported. Denying
    // share-delete would block the delete itself with a sharing violation
    // instead: also contention, but a shape graphox never produces, since
    // nothing in it opens a cache file with a custom share mode.
    let cache_dir = cmd::cache_dir(&temp_dir);
    let held: Vec<std::fs::File> = std::fs::read_dir(&cache_dir)
        .expect("codegen should have written a schema cache")
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| std::fs::File::open(e.path()).expect("could not hold a cache file open"))
        .collect();
    assert!(
        !held.is_empty(),
        "expected at least one cache entry to hold"
    );

    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");

    assert_command_succeeded(&output, "codegen --clean", &temp_dir);
    assert!(
        !generated_dir.exists(),
        "generated directory should be removed even with the cache held open"
    );

    // Success alone would also be what a clean that met no contention returns,
    // so on Windows pin the contention itself: the open handles make the cache
    // directory outlive a removal that was asked for and refused. Unix removes
    // an open file without complaint, so there is nothing to survive.
    #[cfg(windows)]
    assert!(
        cache_dir.exists(),
        "held-open entries should have blocked the cache removal, so the clean \
         was tolerating a failure rather than never meeting one"
    );

    drop(held);
    std::fs::remove_dir_all(&cache_dir).ok();
    std::fs::remove_dir_all(temp_dir).ok();
}

/// A generated file that will not go has to reach the exit status. The project
/// arms that use `output_dir` have always propagated that; the arm that removes
/// files individually computed the same answer and dropped it, so the failure
/// went to stderr and the command still exited 0.
#[test]
#[cfg(unix)]
fn test_codegen_clean_reports_output_it_could_not_remove() {
    use std::os::unix::fs::PermissionsExt;

    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_clean_unremovable_output_test");

    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();
    let app_dir = temp_dir.join("app");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::write(app_dir.join("query.graphql"), "query { me { id } }").unwrap();

    // No output_dir, so the per-file removal path is the one that runs. The
    // include root is stripped from the output path, which puts both the
    // generated file and `__generated__` at the base directory.
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "app/**/*.graphql"
"#,
    )
    .unwrap();

    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process");
    assert_command_succeeded(&output, "codegen", &temp_dir);

    let generated = temp_dir.join("query.codegen.ts");
    let generated_dir = temp_dir.join("__generated__");
    assert!(generated.exists(), "codegen should have written output");
    assert!(
        generated_dir.exists(),
        "codegen should have written a __generated__ dir"
    );

    // A directory that cannot be written is a file that cannot be unlinked, so
    // this blocks the per-file removal and the `__generated__` removal at once.
    std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let output = graphox(bin_path, &temp_dir)
        .arg("codegen")
        .arg("--clean")
        .output()
        .expect("Failed to execute process");
    std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(
        generated.exists(),
        "the fixture is wrong if the output was removable after all"
    );
    assert!(
        !output.status.success(),
        "a generated file that survived the clean must fail the command:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Failed to remove"),
        "the failure should say which file it was: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(cmd::cache_dir(&temp_dir)).ok();
    std::fs::remove_dir_all(temp_dir).ok();
}
