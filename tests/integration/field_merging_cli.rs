//! End-to-end coverage for the spec's field merging rule in `graphox check` and
//! `graphox codegen`: conflicts within a document, across type conditions, and
//! through fragments written in other files.

use crate::support::cmd::graphox;
use tempfile::TempDir;

const SCHEMA: &str = "\
type Query { feed: [Entry] }
union Entry = Article | Video
type Author { id: ID! name: String }
type Article { author: Author! title: String }
type Video { author: Author title: String }
";

/// A scratch workspace with one project over `src`, holding `files`.
fn workspace(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join("src").join(name), text).unwrap();
    }
    std::fs::write(
        dir.path().join("graphox.yaml"),
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"src/**/*.{graphql,ts}\"\n    output_dir: \"gen\"\n",
    )
    .unwrap();
    dir
}

fn run(dir: &TempDir, command: &str) -> (String, Option<i32>) {
    let output = graphox(env!("CARGO_BIN_EXE_graphox"), dir.path())
        .arg(command)
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

/// The fragment alone is valid: the conflict exists only in an operation that
/// spreads it, and that operation lives in another file.
const AUTHOR_FRAGMENT: &str = "\
fragment EntryAuthor on Entry {
  ... on Article { author { name } }
  ... on Video { author { name } }
}
";

#[test]
#[ntest::timeout(20000)]
fn a_nullability_conflict_in_a_fragment_from_another_file_fails_check() {
    let dir = workspace(&[
        ("feed.graphql", "query Feed { feed { ...EntryAuthor } }\n"),
        ("author.graphql", AUTHOR_FRAGMENT),
    ]);

    let (out, code) = run(&dir, "check");
    assert_eq!(code, Some(1), "{out}");
    // Reported in the operation's file, on the `...EntryAuthor` spread, and
    // quoting the fragment's own file with the type each side returns.
    assert!(out.contains("feed.graphql"), "{out}");
    assert!(out.contains("[1:21]"), "{out}");
    assert!(
        out.contains("must not select different types using the same name `author`"),
        "{out}"
    );
    assert!(out.contains("author.graphql:3"), "{out}");
    assert!(out.contains("`Article.author: Author!`"), "{out}");
    assert!(out.contains("`Video.author: Author`"), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn a_conflict_in_the_document_itself_is_reported_on_the_field() {
    let dir = workspace(&[(
        "page.ts",
        "import { graphql } from './gql';\n\
         export const Doc = graphql(`\n  query Feed {\n    feed {\n      ... on Article { author { id } }\n      ... on Video { author { id } }\n    }\n  }\n`);\n",
    )]);

    let (out, code) = run(&dir, "check");
    assert_eq!(code, Some(1), "{out}");
    // Line 6, column 22: the second `author`, where it is written in the
    // TypeScript file.
    assert!(out.contains("[6:22]"), "{out}");
    assert!(
        out.contains("must not select different types using the same name `author`"),
        "{out}"
    );
}

#[test]
#[ntest::timeout(20000)]
fn fields_that_agree_on_type_merge_across_type_conditions() {
    let dir = workspace(&[(
        "feed.graphql",
        "query Feed { feed { ... on Article { title } ... on Video { title } } }\n",
    )]);

    let (out, code) = run(&dir, "check");
    assert_eq!(code, Some(0), "{out}");
}

#[test]
#[ntest::timeout(20000)]
fn codegen_refuses_an_operation_whose_fragments_conflict() {
    let dir = workspace(&[
        ("feed.graphql", "query Feed { feed { ...EntryAuthor } }\n"),
        ("author.graphql", AUTHOR_FRAGMENT),
    ]);

    let (out, code) = run(&dir, "codegen");
    assert_ne!(code, Some(0), "{out}");
    assert!(
        out.contains("must not select different types using the same name `author`"),
        "{out}"
    );
}
