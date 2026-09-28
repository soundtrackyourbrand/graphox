//! End-to-end coverage for `graphox analyze expand`: the document it prints,
//! and how a path picks the project that resolves it.

use crate::support::cmd::graphox;
use tempfile::TempDir;

const SCHEMA: &str = "\
type Query { account(id: ID!): Account, zone(id: ID!): SoundZone }
type Account { id: ID!, name: String, zones: [SoundZone!]! }
type SoundZone { id: ID!, name: String }
";

/// Two projects on one schema, each with its own `ZoneFields`, so the same
/// operation expands differently depending on the project resolving it.
/// `Account` reaches `ZoneFields` only through `AccountFields`, and each
/// definition sits in its own file, one of them embedded in TypeScript.
fn workspace(codegen: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    for app in ["one", "two"] {
        std::fs::create_dir_all(dir.path().join("apps").join(app)).unwrap();
    }
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(
        dir.path().join("apps/one/account.ts"),
        "import { gql } from '@apollo/client';\n\
         export const ACCOUNT = gql`\n  query Account($id: ID!) { account(id: $id) { ...AccountFields } }\n`;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/one/account_fields.graphql"),
        "fragment AccountFields on Account { id owner: name zones { ...ZoneFields } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/one/zone_fields.graphql"),
        "fragment ZoneFields on SoundZone { id }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/two/zone.graphql"),
        "query Zone { zone(id: \"1\") { ...ZoneFields } }\n\
         fragment ZoneFields on SoundZone { id name }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/one/zone.graphql"),
        "query Zone { zone(id: \"1\") { ...ZoneFields } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("graphox.yaml"),
        format!(
            "{codegen}projects:\n  - schema: schema.graphql\n    include: \"apps/one/**/*.{{graphql,ts}}\"\n  - schema: schema.graphql\n    include: \"apps/two/**/*.graphql\"\n"
        ),
    )
    .unwrap();
    dir
}

struct Output {
    stdout: String,
    stderr: String,
    status: Option<i32>,
}

fn run(dir: &TempDir, args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_graphox");
    let output = graphox(bin, dir.path())
        .arg("analyze")
        .arg("expand")
        .args(args)
        .output()
        .unwrap();
    Output {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code(),
    }
}

const ACCOUNT: &str = "\
query Account($id: ID!) {
  account(id: $id) {
    ...AccountFields
  }
}

fragment AccountFields on Account {
  id
  owner: name
  zones {
    ...ZoneFields
  }
}

fragment ZoneFields on SoundZone {
  id
}
";

/// The document is all stdout carries, so it can be piped straight on.
#[test]
#[ntest::timeout(20000)]
fn prints_the_operation_with_every_fragment_it_reaches() {
    let dir = workspace("");
    let out = run(&dir, &["Account"]);

    assert_eq!(out.status, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout, ACCOUNT);
}

#[test]
#[ntest::timeout(20000)]
fn follows_the_configuration_codegen_emits_with() {
    let dir = workspace("codegen:\n  inline_fragments: true\n  emit_ast_aliases: false\n");
    let out = run(&dir, &["Account"]);

    assert_eq!(out.status, Some(0), "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "query Account($id: ID!) {\n  account(id: $id) {\n    id\n    name\n    zones {\n      id\n    }\n  }\n}\n"
    );
}

#[test]
#[ntest::timeout(20000)]
fn json_is_the_request_body_a_client_posts() {
    let dir = workspace("");
    let out = run(&dir, &["Account", "--json"]);

    let body: serde_json::Value = serde_json::Deserializer::from_str(&out.stdout)
        .into_iter()
        .next()
        .expect("a JSON value")
        .unwrap();
    assert_eq!(body["operationName"], "Account");
    assert_eq!(body["query"].as_str().unwrap(), ACCOUNT.trim_end());
}

/// Each project resolves `ZoneFields` to its own copy, so without a path there
/// is no one answer.
#[test]
#[ntest::timeout(20000)]
fn an_operation_that_expands_differently_per_project_needs_a_path() {
    let dir = workspace("");
    let out = run(&dir, &["Zone"]);

    assert_eq!(out.status, Some(1), "{}", out.stdout);
    assert!(out.stderr.contains("expands differently"), "{}", out.stderr);
    assert!(
        out.stderr.contains("apps/one/zone.graphql"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("apps/two/zone.graphql"),
        "{}",
        out.stderr
    );
    assert!(out.stdout.is_empty(), "{}", out.stdout);
}

#[test]
#[ntest::timeout(20000)]
fn a_project_directory_picks_that_projects_fragments() {
    let dir = workspace("");
    let out = run(&dir, &["Zone", "apps/two"]);

    assert_eq!(out.status, Some(0), "{}", out.stderr);
    assert!(
        out.stdout
            .contains("fragment ZoneFields on SoundZone {\n  id\n  name\n}"),
        "{}",
        out.stdout
    );
}

/// A file names its project, and the operation is found anywhere in it — here
/// in a different file from the one given.
#[test]
#[ntest::timeout(20000)]
fn a_file_resolves_the_project_it_belongs_to() {
    let dir = workspace("");
    let out = run(&dir, &["Zone", "apps/one/zone_fields.graphql"]);

    assert_eq!(out.status, Some(0), "{}", out.stderr);
    assert!(
        out.stdout
            .contains("fragment ZoneFields on SoundZone {\n  id\n}"),
        "{}",
        out.stdout
    );
}

#[test]
#[ntest::timeout(20000)]
fn an_unknown_operation_is_an_error() {
    let dir = workspace("");
    let out = run(&dir, &["Nope"]);

    assert_eq!(out.status, Some(1), "{}", out.stdout);
    assert!(
        out.stderr.contains("no operation named 'Nope'"),
        "{}",
        out.stderr
    );
}

#[test]
#[ntest::timeout(20000)]
fn a_path_no_project_includes_is_an_error() {
    let dir = workspace("");
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    let out = run(&dir, &["Account", "docs"]);

    assert_eq!(out.status, Some(1), "{}", out.stdout);
    assert!(out.stderr.contains("no project includes"), "{}", out.stderr);
}

/// A document missing a fragment is one the server rejects, so it is not
/// printed as though it were the request.
#[test]
#[ntest::timeout(20000)]
fn a_spread_that_does_not_resolve_is_an_error() {
    let dir = workspace("");
    std::fs::write(
        dir.path().join("apps/two/broken.graphql"),
        "query Broken { zone(id: \"1\") { ...Missing } }\n",
    )
    .unwrap();
    let out = run(&dir, &["Broken"]);

    assert_eq!(out.status, Some(1), "{}", out.stdout);
    assert!(out.stderr.contains("Missing"), "{}", out.stderr);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
}
