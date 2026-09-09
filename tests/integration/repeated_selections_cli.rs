//! End-to-end coverage for the `repeated_selections` rule: the wiring between
//! the analysis, `--fail-on`, and each reporter's way of carrying a finding
//! that covers several places.

use crate::support::cmd::graphox;

const SCHEMA: &str = "\
type Query { user(id: ID!): User, account(id: ID!): Account }
type User { id: ID!, name: String, email: String, avatar: String }
type Account { id: ID!, owner: User, admin: User }
";

const OPS: &str = "\
fragment UserCard on User { name email }

query A { account(id: \"1\") { owner { name email } } }

query B { account(id: \"2\") { admin { name email avatar } } }

query C { user(id: \"3\") { name email avatar } }
";

fn workspace(name: &str, rules: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(name);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(dir.join("ops.graphql"), OPS).unwrap();
    std::fs::write(
        dir.join("graphox.yaml"),
        format!(
            "projects:\n  - schema: \"schema.graphql\"\n    include: \"ops.graphql\"\nrules:\n  repeated_selections:\n{rules}"
        ),
    )
    .unwrap();
    dir
}

#[test]
#[ntest::timeout(10000)]
fn severity_decides_reporting_and_fail_on_decides_the_exit_code() {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let dir = workspace(
        "graphox_repeated_selections_severity",
        "    - kind: matches_fragment\n      min_fields: 2\n      severity: warning\n",
    );

    let output = graphox(bin, &dir).arg("check").output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("is exactly fragment 'UserCard'"),
        "expected the finding, got:\n{combined}"
    );
    // Default --fail-on is warning, so a warning still ends the run non-zero.
    assert_eq!(output.status.code(), Some(1), "{combined}");

    let output = graphox(bin, &dir)
        .arg("check")
        .arg("--fail-on")
        .arg("error")
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("is exactly fragment 'UserCard'"),
        "the finding is still reported:\n{combined}"
    );
    assert_eq!(output.status.code(), Some(0), "{combined}");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(10000)]
fn an_info_finding_is_reported_without_verbose() {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let dir = workspace(
        "graphox_repeated_selections_info",
        "    - kind: new_fragment\n      min_fields: 2\n      min_uses: 2\n      severity: info\n",
    );

    let output = graphox(bin, &dir).arg("check").output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Consider a fragment"),
        "a rule configured to report at info still reports:\n{combined}"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "info does not fail{combined}"
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(10000)]
fn a_shape_is_one_finding_listing_every_place() {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let dir = workspace(
        "graphox_repeated_selections_shape",
        "    - kind: new_fragment\n      min_fields: 3\n      min_uses: 2\n      severity: warning\n",
    );

    let output = graphox(bin, &dir).arg("check").output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // `{ name email avatar }` recurs in B and C: one finding, not two.
    let findings = combined.matches("Consider a fragment").count();
    assert_eq!(findings, 1, "one shape is one finding:\n{combined}");
    assert!(
        combined.contains("also selected in"),
        "the other place is listed:\n{combined}"
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(10000)]
fn reporters_that_carry_one_location_name_the_others_in_the_message() {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let dir = workspace(
        "graphox_repeated_selections_reporters",
        "    - kind: new_fragment\n      min_fields: 3\n      min_uses: 2\n      severity: warning\n",
    );

    for reporter in ["github", "tsc"] {
        let output = graphox(bin, &dir)
            .arg("check")
            .arg("--reporter")
            .arg(reporter)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Consider a fragment"),
            "{reporter} reported the finding:\n{stdout}"
        );
        assert!(
            stdout.contains("Also at:"),
            "{reporter} named the other places in the message:\n{stdout}"
        );
        // One line per finding: the annotation formats are line-oriented.
        assert_eq!(
            stdout.matches("Consider a fragment").count(),
            1,
            "{reporter} emitted it once:\n{stdout}"
        );
    }

    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ntest::timeout(10000)]
fn related_paths_are_relative_to_the_same_root_as_the_primary() {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let dir = std::env::temp_dir().join("graphox_repeated_selections_subdir");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(dir.join("nested/deep")).unwrap();
    std::fs::write(dir.join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(
        dir.join("nested/a.graphql"),
        "query A { account(id: \"1\") { owner { name email avatar } } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("nested/deep/b.graphql"),
        "query B { account(id: \"2\") { admin { name email avatar } } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("graphox.yaml"),
        "projects:\n  - schema: schema.graphql\n    include: \"nested/**/*.graphql\"\nrules:\n  repeated_selections:\n    - kind: new_fragment\n      min_fields: 3\n      min_uses: 2\n      severity: warning\n",
    )
    .unwrap();

    // Run from a subdirectory: graphox walks up to find the config, so the
    // primary path is relative to the config's directory. A related path
    // relativized against the working directory instead would read
    // "b.graphql", which is not reachable from the primary path beside it.
    let output = graphox(bin, dir.join("nested/deep"))
        .arg("check")
        .arg("--fail-on")
        .arg("error")
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("nested/deep/b.graphql") || combined.contains("nested\\deep\\b.graphql"),
        "the related path should be relative to the config root:\n{combined}"
    );

    std::fs::remove_dir_all(dir).ok();
}
