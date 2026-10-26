//! End-to-end coverage for the `field_arguments` rule in `graphox check`:
//! the values a document writes itself, followed across files, with project
//! rules, severities and ignore comments.

use crate::support::cmd::graphox;
use tempfile::TempDir;

const SCHEMA: &str = "\
type Query { playlist(id: ID!): Playlist, album: Album }
type Playlist { tracks(first: Int, market: String): TrackConnection! }
type Album { tracks(first: Int): TrackConnection! }
type TrackConnection { total: Int }
";

const RULES: &str = "\
    rules:
      field_arguments:
        Playlist.tracks:
          first: { max: 100 }
          reason: page through it instead
        Album.tracks:
          first: [100, 500]
          severity: warning
";

/// A scratch workspace with one project over `src`, holding `files`.
fn workspace(files: &[(&str, &str)], rules: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join("src").join(name), text).unwrap();
    }
    std::fs::write(
        dir.path().join("graphox.yaml"),
        format!(
            "projects:\n  - schema: \"schema.graphql\"\n    include: \"src/**/*.{{graphql,ts}}\"\n{rules}"
        ),
    )
    .unwrap();
    dir
}

fn check(dir: &TempDir, args: &[&str]) -> (String, Option<i32>) {
    let output = graphox(env!("CARGO_BIN_EXE_graphox"), dir.path())
        .arg("check")
        .args(args)
        .output()
        .unwrap();
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
fn a_default_is_followed_into_a_fragment_in_another_file() {
    let dir = workspace(
        &[
            (
                "page.graphql",
                "query Page($first: Int = 2500) { playlist(id: \"1\") { ...Tracks } }\n",
            ),
            (
                "tracks.graphql",
                "fragment Tracks on Playlist { tracks(first: $first) { total } }\n",
            ),
        ],
        RULES,
    );

    let (out, code) = check(&dir, &[]);
    assert!(
        out.contains("src/page.graphql")
            && out.contains(
                "'$first' defaults to 2500, which is passed to 'Playlist.tracks(first:)' in \
                 fragment 'Tracks', above its limit of 100: page through it instead"
            ),
        "{out}"
    );
    assert_eq!(code, Some(1), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn literals_are_checked_in_embedded_documents_at_their_own_position() {
    let dir = workspace(
        &[(
            "page.ts",
            "import { graphql } from './gql';\n\
             export const Doc = graphql(`\n  query Page {\n    playlist(id: \"1\") { tracks(first: 3000) { total } }\n  }\n`);\n",
        )],
        RULES,
    );

    let (out, _) = check(&dir, &[]);
    // Line 4, where `3000` is written in the TypeScript file.
    assert!(
        out.contains("[4:36]") && out.contains("'Playlist.tracks(first:)' is 3000 here"),
        "{out}"
    );
}

#[test]
#[ntest::timeout(20000)]
fn a_warning_entry_does_not_fail_with_fail_on_error() {
    let dir = workspace(
        &[(
            "album.graphql",
            "query A { album { tracks(first: 200) { total } } }\n",
        )],
        RULES,
    );

    let (out, code) = check(&dir, &["--fail-on", "error"]);
    assert!(
        out.contains("'Album.tracks(first:)' is 200 here, but it may only be 100 or 500"),
        "{out}"
    );
    assert_eq!(code, Some(0), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn an_ignore_comment_naming_the_rule_silences_it() {
    let dir = workspace(
        &[(
            "page.graphql",
            "query Page {\n  playlist(id: \"1\") { tracks(first: 999) { total } } # graphox-ignore field_arguments: measured\n}\n",
        )],
        RULES,
    );

    let (out, code) = check(&dir, &[]);
    assert!(
        !out.contains("field_argument") && !out.contains("999"),
        "{out}"
    );
    assert_eq!(code, Some(0), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn a_project_without_limits_is_not_checked() {
    let dir = workspace(
        &[(
            "page.graphql",
            "query Page { playlist(id: \"1\") { tracks(first: 3000) { total } } }\n",
        )],
        "",
    );

    let (out, code) = check(&dir, &[]);
    assert_eq!(code, Some(0), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn a_limit_the_schema_cannot_match_is_reported() {
    let dir = workspace(
        &[(
            "page.graphql",
            "query Page { album { tracks { total } } }\n",
        )],
        "    rules:\n      field_arguments:\n        Album.songs:\n          first: { max: 1 }\n        Album.tracks:\n          last: { max: 1 }\n",
    );

    let (out, _) = check(&dir, &[]);
    assert!(
        out.contains("limits 'Album.songs', which the schema does not have"),
        "{out}"
    );
    assert!(
        out.contains("limits 'Album.tracks(last:)', an argument the schema does not have"),
        "{out}"
    );
}
