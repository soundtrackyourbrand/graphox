//! End-to-end coverage for `graphox analyze operations`: the ranking, the
//! drill-down, and the scoping flags.

use crate::support::cmd::graphox;
use tempfile::TempDir;

const SCHEMA: &str = "\
type Query { account(id: ID!): Account, zone(id: ID!): SoundZone }
type Account { id: ID!, name: String, zones: [SoundZone!]! }
type SoundZone { id: ID!, name: String, playlists: [Playlist!]! }
type Playlist { id: ID!, tracks: [Track!]! }
type Track { id: ID!, title: String }
type Mutation { renameZone(id: ID!, name: String!): SoundZone }
";

/// Two projects on one schema, so `--app` has something to narrow to. `Deep`
/// reaches its depth through a fragment, `Shallow` does not, and `Rename` is
/// the only mutation.
fn workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/one")).unwrap();
    std::fs::create_dir_all(dir.path().join("apps/two")).unwrap();
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(
        dir.path().join("apps/one/deep.graphql"),
        "query Deep { account(id: \"1\") { ...AccountTree } }\n\
         fragment AccountTree on Account { zones { playlists { tracks { id title } } } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/one/rename.graphql"),
        "mutation Rename($id: ID!, $name: String!) { renameZone(id: $id, name: $name) { id } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/two/shallow.graphql"),
        "query Shallow { zone(id: \"1\") { id name } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("graphox.yaml"),
        "projects:\n  - schema: schema.graphql\n    include: \"apps/one/**/*.graphql\"\n  - schema: schema.graphql\n    include: \"apps/two/**/*.graphql\"\n",
    )
    .unwrap();
    dir
}

fn run(dir: &TempDir, args: &[&str]) -> (String, Option<i32>) {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let mut cmd = graphox(bin, dir.path());
    cmd.arg("analyze").arg("operations");
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
#[ntest::timeout(20000)]
fn ranks_operations_by_depth() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--limit", "0"]);

    assert!(out.contains("3 operations in scope"), "{out}");
    let deep = out
        .find("Deep")
        .unwrap_or_else(|| panic!("expected Deep:\n{out}"));
    let shallow = out
        .find("Shallow")
        .unwrap_or_else(|| panic!("expected Shallow:\n{out}"));
    assert!(deep < shallow, "the deepest operation comes first:\n{out}");
}

/// The pair of depth columns is the point of the ranking: it separates an
/// operation that is deep by itself from one that is deep through a fragment.
#[test]
#[ntest::timeout(20000)]
fn own_depth_shows_the_depth_a_fragment_contributes() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--name", "Deep"]);

    assert!(out.contains("depth             5"), "{out}");
    assert!(out.contains("(1 in its own body)"), "{out}");
    assert!(
        out.contains("account.zones.playlists.tracks"),
        "the nesting path is named:\n{out}"
    );
}

#[test]
#[ntest::timeout(20000)]
fn sorting_by_lists_leads_with_the_multiplying_request() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--sort", "lists", "--limit", "1"]);

    assert!(out.contains("Deep"), "three nested lists lead:\n{out}");
    assert!(!out.contains("Shallow"), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn kind_selects_one_operation_type() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--kind", "mutation", "--limit", "0"]);

    assert!(out.contains("Rename"), "{out}");
    assert!(!out.contains("Deep"), "queries are excluded:\n{out}");
}

#[test]
#[ntest::timeout(20000)]
fn scoping_to_an_app_narrows_what_is_ranked() {
    let dir = workspace();
    let (out, _) = run(&dir, &["--app", "apps/two", "--limit", "0"]);

    assert!(out.contains("Shallow"), "{out}");
    assert!(!out.contains("Deep"), "{out}");
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
fn an_unknown_kind_is_rejected() {
    let dir = workspace();
    let (out, status) = run(&dir, &["--kind", "nope"]);

    assert_eq!(status, Some(1), "{out}");
    assert!(out.contains("Unknown --kind"), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn json_carries_every_operation_and_its_metrics() {
    let dir = workspace();
    // --limit is a display concern; JSON stays complete.
    let (out, _) = run(&dir, &["--json", "--limit", "1"]);
    let line = out
        .lines()
        .find(|l| l.starts_with('{'))
        .unwrap_or_else(|| panic!("expected a JSON line:\n{out}"));

    assert!(line.contains("\"operations\""), "{line}");
    assert!(line.contains("\"unparsed\""), "{line}");
    assert_eq!(
        line.matches("\"operation\":").count(),
        3,
        "all three survive --limit 1:\n{line}"
    );
    assert!(line.contains("\"depth\":5"), "{line}");
    assert!(line.contains("\"own_depth\":1"), "{line}");
    assert!(line.contains("\"list_nesting\":3"), "{line}");
    assert!(
        line.contains("\"list_path\":\"account.zones.playlists.tracks\""),
        "{line}"
    );
    assert!(line.contains("\"kind\":\"mutation\""), "{line}");
}

fn json_names(out: &str) -> Vec<String> {
    // The output can carry log lines after the document, so read one value.
    let start = out.find('{').expect("JSON output");
    let value: serde_json::Value = serde_json::Deserializer::from_str(&out[start..])
        .into_iter()
        .next()
        .expect("a JSON value")
        .unwrap();
    value["operations"]
        .as_array()
        .expect("an operations array")
        .iter()
        .map(|op| op["operation"].as_str().unwrap().to_string())
        .collect()
}

/// `--sort` orders the JSON the same way it orders the table. The two keys
/// rank this workspace differently, so each order shows its own key applied.
#[test]
#[ntest::timeout(20000)]
fn json_output_follows_the_sort() {
    let dir = workspace();

    let (by_depth, _) = run(&dir, &["--json", "--sort", "depth"]);
    assert_eq!(json_names(&by_depth), ["Deep", "Rename", "Shallow"]);

    let (by_fields, _) = run(&dir, &["--json", "--sort", "fields"]);
    assert_eq!(json_names(&by_fields), ["Deep", "Shallow", "Rename"]);
}

/// Files that did not parse can be why nothing matched, so they are reported
/// with the empty result rather than hidden by it.
#[test]
#[ntest::timeout(20000)]
fn an_empty_result_still_reports_files_that_did_not_parse() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/one")).unwrap();
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(
        dir.path().join("apps/one/broken.graphql"),
        "}} this is not GraphQL {{\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("graphox.yaml"),
        "projects:\n  - schema: schema.graphql\n    include: \"apps/one/**/*.graphql\"\n",
    )
    .unwrap();

    let (out, _) = run(&dir, &[]);
    assert!(out.contains("No operation matched."), "{out}");
    assert!(out.contains("1 file did not parse"), "{out}");
}
