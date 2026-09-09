//! End-to-end coverage for `graphox analyze usage`: the scoping flags and the
//! three shapes of output they select between.

use crate::support::cmd::graphox;
use tempfile::TempDir;

const SCHEMA: &str = "\
type Query { account(id: ID!): Account, zone(id: ID!): SoundZone }
type Account { id: ID!, name: String, legacyFlag: Boolean }
type SoundZone { id: ID!, name: String }
";

/// Two projects on one schema, so `--app` has something to narrow to.
fn workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/one")).unwrap();
    std::fs::create_dir_all(dir.path().join("apps/two")).unwrap();
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(
        dir.path().join("apps/one/a.graphql"),
        "query A { account(id: \"1\") { id name } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/two/b.graphql"),
        "query B { account(id: \"2\") { id } zone(id: \"1\") { id } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("graphox.yaml"),
        "projects:\n  - schema: schema.graphql\n    include: \"apps/one/**/*.graphql\"\n  - schema: schema.graphql\n    include: \"apps/two/**/*.graphql\"\n",
    )
    .unwrap();
    dir
}

/// Runs `analyze usage`, returning the merged output and the exit status.
fn run(dir: &TempDir, args: &[&str]) -> (String, Option<i32>) {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let mut cmd = graphox(bin, dir.path());
    cmd.arg("analyze").arg("usage");
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

#[test]
#[ntest::timeout(10000)]
fn lists_types_ranked_by_consumers() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--limit", "0"]);

    assert!(out.contains("Account"), "{out}");
    assert!(out.contains("SoundZone"), "{out}");
    // Account is selected by both operations, SoundZone by one.
    let account = out.find("Account").unwrap();
    let zone = out.find("SoundZone").unwrap();
    assert!(account < zone, "more-used types come first:\n{out}");
}

#[test]
#[ntest::timeout(10000)]
fn a_named_type_lists_its_fields_by_consumer_count() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--type", "Account", "--limit", "0"]);

    assert!(out.contains("2 of 3 fields used"), "{out}");
    // `id` in both operations, `name` in one, `legacyFlag` in neither.
    let id = out
        .find("   2  id")
        .unwrap_or_else(|| panic!("expected id count:\n{out}"));
    let name = out
        .find("   1  name")
        .unwrap_or_else(|| panic!("expected name count:\n{out}"));
    let legacy = out
        .find("   0  legacyFlag")
        .unwrap_or_else(|| panic!("expected legacyFlag count:\n{out}"));
    assert!(id < name && name < legacy, "ordered by count:\n{out}");
}

#[test]
#[ntest::timeout(10000)]
fn scoping_to_an_app_narrows_the_counts() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--app", "apps/one", "--type", "Account"]);

    // Only query A is in scope, so `id` drops from two consumers to one and
    // `name` is still selected.
    assert!(out.contains("   1  id"), "{out}");
    assert!(out.contains("   1  name"), "{out}");
    assert!(
        !out.contains("   2  id"),
        "app scope should exclude B:\n{out}"
    );
}

#[test]
#[ntest::timeout(10000)]
fn a_named_field_lists_what_selects_it() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--type", "Account", "--field", "name"]);

    assert!(out.contains("Account.name"), "{out}");
    assert!(out.contains("1 consumer"), "{out}");
    // The consumer line names the operation and its file together; asserting on
    // the operation alone would match the "A" inside "Account.name".
    let consumer = out
        .lines()
        .find(|line| line.trim_start().starts_with("A  "))
        .unwrap_or_else(|| panic!("expected a consumer line for query A:\n{out}"));
    assert!(
        consumer.contains("apps/one/a.graphql") || consumer.contains("apps\\one\\a.graphql"),
        "the consumer's file is named beside it:\n{out}"
    );
}

#[test]
#[ntest::timeout(10000)]
fn unused_reports_declared_fields_nothing_selects() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--unused", "--limit", "0"]);

    assert!(out.contains("legacyFlag"), "{out}");
    // A field two operations select is not unused.
    assert!(!out.contains("Account.id"), "{out}");
}

#[test]
#[ntest::timeout(10000)]
fn an_unknown_app_names_the_projects_that_exist() {
    let dir = workspace();
    let (out, status) = run(&dir, &["--app", "nope"]);

    assert_eq!(status, Some(1), "an unmatched scope is an error:\n{out}");
    assert!(out.contains("no project matched"), "{out}");
    assert!(
        out.contains("apps/one"),
        "the known projects are listed:\n{out}"
    );
}

#[test]
#[ntest::timeout(10000)]
fn json_carries_every_field_and_its_consumers() {
    let dir = workspace();
    // --limit is a display concern; JSON stays complete.
    let (out, _) = run(&dir, &["--json", "--type", "Account", "--limit", "1"]);
    let line = out
        .lines()
        .find(|l| l.starts_with('{'))
        .unwrap_or_else(|| panic!("expected a JSON line:\n{out}"));

    assert!(line.contains("\"fields\""), "{line}");
    assert!(line.contains("\"unparsed\""), "{line}");
    assert!(line.contains("\"legacyFlag\""), "{line}");
    assert!(line.contains("\"consumer_count\":2"), "{line}");
    assert_eq!(
        line.matches("\"field\":").count(),
        3,
        "all three fields survive --limit 1:\n{line}"
    );
}
