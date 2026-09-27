//! `check [PATH]` and `codegen [PATH]`: the path limits what is reported or
//! generated, within the workspace found from the current directory.

use std::path::{Path, PathBuf};
use std::process::Output;

use crate::support::cmd::{assert_command_succeeded, fresh_dir, graphox};

const SCHEMA_A: &str = "type Query { me: User } type User { id: ID! name: String }";
const SCHEMA_B: &str = "type Query { version: String }";

const CONFIG: &str = r#"
schema_types:
  - schema: packages/a/schema.graphql
    output: packages/a/codegen/schema.ts
    possible_types: packages/a/codegen/possible-types.ts
  - schema: packages/b/schema.graphql
    output: packages/b/codegen/schema.ts
projects:
  - include: apps/one/src
    schema: packages/a/schema.graphql
    output_dir: apps/one/graphql
  - include: apps/two/src
    schema: packages/a/schema.graphql
    output_dir: apps/shared/graphql
  - include: apps/extra/src
    schema: packages/a/schema.graphql
    output_dir: apps/shared/graphql
"#;

/// Two schema packages and three projects, two of which share an output
/// directory.
fn workspace(name: &str) -> PathBuf {
    let dir = fresh_dir(name);
    let write = |rel: &str, content: &str| {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    };
    write("graphox.yaml", CONFIG);
    write("packages/a/schema.graphql", SCHEMA_A);
    write("packages/b/schema.graphql", SCHEMA_B);
    write("apps/one/src/one.graphql", "query One { me { id } }");
    write(
        "apps/one/src/nested/deep.graphql",
        "query Deep { me { name } }",
    );
    write("apps/two/src/two.graphql", "query Two { me { id } }");
    write(
        "apps/extra/src/extra.graphql",
        "query Extra { me { name } }",
    );
    write("docs/readme.md", "not graphql");
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    graphox(env!("CARGO_BIN_EXE_graphox"), dir)
        .args(args)
        .output()
        .expect("Failed to execute process")
}

fn exists(dir: &Path, rel: &str) -> bool {
    dir.join(rel).exists()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Everything the command printed. Diagnostics and progress go to different
/// streams depending on the reporter.
fn printed(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

#[test]
#[ntest::timeout(20000)]
fn codegen_for_a_schema_package_writes_only_its_schema_types() {
    let dir = workspace("graphox_scope_schema_package");
    // A project that would fail if it were processed: it matches no documents.
    std::fs::write(
        dir.join("graphox.yaml"),
        format!("{CONFIG}  - include: apps/empty/src\n    schema: packages/a/schema.graphql\n"),
    )
    .unwrap();

    let output = run(&dir, &["codegen", "packages/a"]);
    assert_command_succeeded(&output, "codegen packages/a", &dir);

    assert!(exists(&dir, "packages/a/codegen/schema.ts"));
    assert!(exists(&dir, "packages/a/codegen/possible-types.ts"));
    assert!(!exists(&dir, "packages/b/codegen/schema.ts"));
    assert!(!exists(&dir, "apps/one/graphql"), "no project generated");
    assert!(!exists(&dir, "apps/shared/graphql"), "no project generated");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn codegen_for_a_project_writes_only_that_project() {
    let dir = workspace("graphox_scope_project");

    let output = run(&dir, &["codegen", "apps/one"]);
    assert_command_succeeded(&output, "codegen apps/one", &dir);

    let entrypoint = std::fs::read_to_string(dir.join("apps/one/graphql/graphql.ts")).unwrap();
    assert!(entrypoint.contains("One") && entrypoint.contains("Deep"));
    assert!(!exists(&dir, "apps/shared/graphql"));
    assert!(!exists(&dir, "packages/a/codegen/schema.ts"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn a_path_inside_a_project_generates_the_whole_project() {
    let dir = workspace("graphox_scope_inside_project");

    let output = run(&dir, &["codegen", "apps/one/src/nested"]);
    assert_command_succeeded(&output, "codegen apps/one/src/nested", &dir);

    let entrypoint = std::fs::read_to_string(dir.join("apps/one/graphql/graphql.ts")).unwrap();
    assert!(entrypoint.contains("One") && entrypoint.contains("Deep"));
    std::fs::remove_dir_all(dir).ok();
}

/// An output directory's entrypoint lists every project writing there, so
/// generating one of them regenerates them all.
#[test]
#[ntest::timeout(20000)]
fn projects_sharing_an_output_directory_are_generated_together() {
    let dir = workspace("graphox_scope_shared_output");

    let output = run(&dir, &["codegen", "apps/two"]);
    assert_command_succeeded(&output, "codegen apps/two", &dir);

    let entrypoint = std::fs::read_to_string(dir.join("apps/shared/graphql/graphql.ts")).unwrap();
    assert!(entrypoint.contains("Two"), "{entrypoint}");
    assert!(entrypoint.contains("Extra"), "{entrypoint}");
    assert!(!exists(&dir, "apps/one/graphql"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn codegen_without_a_path_generates_everything() {
    let dir = workspace("graphox_scope_none");

    let output = run(&dir, &["codegen"]);
    assert_command_succeeded(&output, "codegen", &dir);

    for rel in [
        "packages/a/codegen/schema.ts",
        "packages/b/codegen/schema.ts",
        "apps/one/graphql/graphql.ts",
        "apps/shared/graphql/graphql.ts",
    ] {
        assert!(exists(&dir, rel), "{rel}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn codegen_fails_for_a_path_with_nothing_to_generate() {
    let dir = workspace("graphox_scope_nothing");

    let nothing = run(&dir, &["codegen", "docs"]);
    assert!(!nothing.status.success());
    assert!(stderr(&nothing).contains("'docs' holds no project and no schema_types output"));

    let missing = run(&dir, &["codegen", "packages/typo"]);
    assert!(!missing.status.success());
    assert!(stderr(&missing).contains("Path 'packages/typo' cannot be used"));

    assert!(
        !exists(&dir, "packages/a/codegen/schema.ts"),
        "nothing was generated"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn a_path_outside_the_workspace_is_rejected() {
    let dir = workspace("graphox_scope_outside");
    let outside = fresh_dir("graphox_scope_outside_other");

    let output = run(&dir, &["codegen", outside.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("is outside the workspace"),
        "{}",
        stderr(&output)
    );
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(outside).ok();
}

/// `--clean` removes every generated file; with a path it would remove more
/// than the path suggests.
#[test]
#[ntest::timeout(20000)]
fn clean_does_not_take_a_path() {
    let dir = workspace("graphox_scope_clean");
    assert_command_succeeded(&run(&dir, &["codegen"]), "codegen", &dir);

    let output = run(&dir, &["codegen", "--clean", "packages/a"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("does not take a path"));
    assert!(
        exists(&dir, "packages/b/codegen/schema.ts"),
        "nothing was removed"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn check_reports_only_files_under_the_path() {
    let dir = workspace("graphox_scope_check");
    // An invalid field in apps/two only.
    std::fs::write(
        dir.join("apps/two/src/two.graphql"),
        "query Two { me { missing } }",
    )
    .unwrap();

    let elsewhere = run(&dir, &["check", "apps/one"]);
    assert_command_succeeded(&elsewhere, "check apps/one", &dir);
    assert!(
        !stdout(&elsewhere).contains("missing"),
        "{}",
        stdout(&elsewhere)
    );

    let here = run(&dir, &["check", "apps/two"]);
    assert!(
        !here.status.success(),
        "the error under the path fails the run"
    );
    assert!(printed(&here).contains("missing"), "{}", printed(&here));

    let everything = run(&dir, &["check"]);
    assert!(!everything.status.success());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(20000)]
fn check_fails_for_a_path_without_documents() {
    let dir = workspace("graphox_scope_check_nothing");

    let output = run(&dir, &["check", "docs"]);
    assert!(!output.status.success());
    let text = format!("{}{}", stdout(&output), stderr(&output));
    assert!(
        text.contains("'docs' holds no document of any project"),
        "{text}"
    );
    std::fs::remove_dir_all(dir).ok();
}
