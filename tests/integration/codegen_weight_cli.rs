//! End-to-end coverage for `graphox analyze codegen`.
//!
//! The load-bearing test here is [`totals_reconcile_with_the_files_codegen_writes`]:
//! the numbers are only worth reading if they add up to the real output, and
//! nothing else in the suite would notice if they stopped.

use crate::support::cmd::graphox;
use tempfile::TempDir;

const SCHEMA: &str = "\
type Query { account(id: ID!): Account, zone(id: ID!): SoundZone }
type Account { id: ID!, name: String, zones: [SoundZone!]! }
type SoundZone { id: ID!, name: String }
";

/// Two projects on one schema. `Shared` is spread twice so its leverage is
/// more than one, and `generate_ast_for_fragments` is on so fragments carry a
/// document AST as they do in a real workspace.
fn workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/one")).unwrap();
    std::fs::create_dir_all(dir.path().join("apps/two")).unwrap();
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(
        dir.path().join("apps/one/a.graphql"),
        "query A { account(id: \"1\") { ...Shared } }\n\
         query B { account(id: \"2\") { ...Shared name } }\n\
         fragment Shared on Account { id zones { id name } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/two/b.graphql"),
        "query C { zone(id: \"1\") { id name } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("graphox.yaml"),
        "codegen:\n  generate_ast_for_fragments: true\n\
         projects:\n  - schema: schema.graphql\n    include: \"apps/one/**/*.graphql\"\n    output_dir: apps/one/generated\n  - schema: schema.graphql\n    include: \"apps/two/**/*.graphql\"\n    output_dir: apps/two/generated\n",
    )
    .unwrap();
    dir
}

fn run_with(dir: &TempDir, command: &str, args: &[&str]) -> (String, Option<i32>) {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let mut cmd = graphox(bin, dir.path());
    cmd.arg("analyze").arg(command);
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().unwrap();
    (
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        output.status.code(),
    )
}

fn run(dir: &TempDir, args: &[&str]) -> (String, Option<i32>) {
    run_with(dir, "codegen", args)
}

fn json_line(out: &str) -> &str {
    out.lines()
        .find(|l| l.starts_with('{'))
        .unwrap_or_else(|| panic!("expected a JSON line:\n{out}"))
}

/// Every integer field of one name in a JSON line.
fn numbers(line: &str, field: &str) -> Vec<usize> {
    let needle = format!("\"{field}\":");
    line.match_indices(&needle)
        .map(|(at, _)| {
            let tail = &line[at + needle.len()..];
            let end = tail
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(tail.len());
            tail[..end].parse().unwrap_or_else(|_| {
                panic!("{field} at {at} was not a number in:\n{line}");
            })
        })
        .collect()
}

/// The attribution's whole claim: per-definition bytes plus each file's shared
/// preamble come to the bytes codegen actually wrote. A generator change that
/// emitted something no definition was charged for would land here.
#[test]
#[ntest::timeout(20000)]
fn totals_reconcile_with_the_files_codegen_writes() {
    let dir = workspace();

    let bin = env!("CARGO_BIN_EXE_graphox");
    let generated = graphox(bin, dir.path()).arg("codegen").output().unwrap();
    assert!(
        generated.status.success(),
        "codegen failed: {}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let mut on_disk = 0u64;
    let mut files = 0;
    for project in ["apps/one/generated", "apps/two/generated"] {
        for entry in std::fs::read_dir(dir.path().join(project)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            // Only the per-source outputs: `index.ts` and `fragment-masking.ts`
            // are written per directory, not generated from a definition.
            if name.ends_with(".codegen.ts") {
                on_disk += entry.metadata().unwrap().len();
                files += 1;
            }
        }
    }
    assert_eq!(files, 2, "one output per source file");

    let (out, _) = run(&dir, &["--json"]);
    let line = json_line(&out);
    let attributed: usize = numbers(line, "generated_bytes").iter().sum::<usize>() / 2;
    let shared: usize = numbers(line, "shared_bytes").iter().sum();

    // `generated_bytes` appears on both the definition rows and the project
    // rollups, so the raw sum double-counts; halving it also checks that the
    // two agree.
    assert_eq!(
        attributed + shared,
        on_disk as usize,
        "attributed {attributed} + shared {shared} should be the {on_disk} bytes on disk:\n{line}"
    );
}

#[test]
#[ntest::timeout(20000)]
fn reports_a_definition_with_its_bytes_and_leverage() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--limit", "0"]);

    assert!(out.contains("4 definitions"), "{out}");
    assert!(out.contains("Shared"), "{out}");
    assert!(out.contains("fragment"), "{out}");
    // The per-project rollup follows the definition table.
    assert!(out.contains("PROJECT"), "{out}");
    assert!(out.contains("apps/one"), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn a_fragment_records_the_definitions_that_spread_it() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--kind", "fragment", "--json"]);
    let line = json_line(&out);

    assert!(line.contains("\"definition\":\"Shared\""), "{line}");
    assert!(
        line.contains("\"spread_by\":2"),
        "A and B both spread it:\n{line}"
    );
}

/// An operation is spread by nothing, which is not the same as being spread by
/// nobody — the column has no meaning for it.
#[test]
#[ntest::timeout(20000)]
fn an_operation_has_no_spread_count() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--kind", "operation", "--json"]);
    let line = json_line(&out);

    assert!(line.contains("\"spread_by\":null"), "{line}");
    assert!(!line.contains("\"kind\":\"fragment\""), "{line}");
}

#[test]
#[ntest::timeout(20000)]
fn the_ast_is_reported_apart_from_the_types() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--json"]);
    let line = json_line(&out);

    let generated = numbers(line, "generated_bytes");
    let ast = numbers(line, "ast_bytes");
    assert!(!ast.is_empty(), "{line}");
    assert!(
        ast.iter().all(|bytes| *bytes > 0),
        "generate_ast_for_fragments is on, so everything has one:\n{line}"
    );
    assert!(
        generated.iter().zip(&ast).all(|(total, ast)| ast < total),
        "the AST is a part of what a definition generates:\n{line}"
    );
}

#[test]
#[ntest::timeout(20000)]
fn scoping_to_an_app_narrows_the_totals() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--app", "apps/two", "--limit", "0"]);

    assert!(out.contains("C"), "{out}");
    assert!(!out.contains("Shared"), "{out}");
    assert!(out.contains("1 project"), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn an_unknown_app_names_the_projects_that_exist() {
    let dir = workspace();
    let (out, status) = run(&dir, &["--app", "nope"]);

    assert_eq!(status, Some(1), "an unmatched scope is an error:\n{out}");
    assert!(out.contains("no project matched"), "{out}");
    assert!(out.contains("apps/one"), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn an_unknown_sort_is_rejected() {
    let dir = workspace();
    let (out, status) = run(&dir, &["--sort", "nope"]);

    assert_eq!(status, Some(1), "{out}");
    assert!(out.contains("Unknown --sort"), "{out}");
}

/// Measuring must not write: someone reading the numbers has not asked for
/// their working tree to change.
#[test]
#[ntest::timeout(20000)]
fn measuring_the_output_does_not_write_it() {
    let dir = workspace();
    let (out, status) = run(&dir, &["--limit", "0"]);

    assert_eq!(status, Some(0), "{out}");
    assert!(
        !dir.path().join("apps/one/generated").exists(),
        "no output directory should have been created:\n{out}"
    );
}
