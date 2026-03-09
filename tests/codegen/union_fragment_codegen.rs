use std::process::Command;

fn run_codegen_fixture(test_name: &str, schema: &str, query: &str, config: &str) -> String {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join(test_name);
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    std::fs::write(temp_dir.join("schema.graphql"), schema).unwrap();
    std::fs::write(temp_dir.join("query.ts"), query).unwrap();
    std::fs::write(temp_dir.join("graphox.yaml"), config).unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let content = std::fs::read_to_string(temp_dir.join("query.codegen.ts")).unwrap();
    std::fs::remove_dir_all(temp_dir).ok();
    content
}

#[test]
#[ntest::timeout(1000)]
fn test_union_fragment_codegen() {
    let content = run_codegen_fixture(
        "graphox_union_fragment_test",
        r#"
union PlaybackSource = Playlist | Schedule | Soundtrack

type Playlist {
  id: ID!
  title: String!
}

type Schedule {
  id: ID!
  time: String!
}

type Soundtrack {
  id: ID!
  artist: String!
}

type Query {
  playFrom: PlaybackSource
}
"#,
        r#"
const q = gql`
  fragment PlaylistInfo on Playlist {
    title
  }

  fragment ScheduleInfo on Schedule {
    time
  }

  query TestQuery {
    playFrom {
      ...PlaylistInfo
      ...ScheduleInfo
    }
  }
`;
"#,
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
    output_dir: "."
"#,
    );

    // Current (presumably buggy) output might look like:
    // playFrom: ({ __typename: "PlaybackSource" } & PlaylistInfo & ScheduleInfo) | null;

    // We want it to be more like a union:
    // playFrom: PlaylistInfo | ScheduleInfo | { __typename: "Soundtrack" } | null;

    // Check if it's using intersection (buggy behavior)
    assert!(
        !content.contains("& PlaylistInfo & ScheduleInfo"),
        "Should not use intersection for fragments on different types of a union. Content:\n{}",
        content
    );

    // Check if it includes Soundtrack (the missing member)
    assert!(
        content.contains("__typename: \"Soundtrack\""),
        "Should include unhandled union members. Content:\n{}",
        content
    );
}

#[test]
#[ntest::timeout(1000)]
fn test_union_fragment_codegen_merge_keys_use_applicable_spreads() {
    let content = run_codegen_fixture(
        "graphox_union_fragment_merge_key_test",
        r#"
union PlaybackSource = Playlist | Schedule | Soundtrack

type Playlist {
  id: ID!
  title: String!
}

type Schedule {
  id: ID!
  time: String!
}

type Soundtrack {
  id: ID!
  artist: String!
}

type Query {
  playFrom: PlaybackSource
}
"#,
        r#"
const q = gql`
  fragment PlaylistInfo on Playlist {
    title
  }

  fragment ScheduleInfo on Schedule {
    time
  }

  query TestQuery {
    playFrom {
      ...PlaylistInfo
      ...ScheduleInfo
    }
  }
`;
"#,
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
    output_dir: "."
    codegen:
      merge_union_types: true
"#,
    );

    assert!(
        !content.contains("__typename: \"Playlist\" | \"Schedule\""),
        "Members with different applicable fragment spreads should not be grouped. Content:\n{}",
        content
    );
    assert!(
        content.contains("__typename: \"Playlist\";"),
        "Playlist branch should be preserved. Content:\n{}",
        content
    );
    assert!(
        content.contains("__typename: \"Schedule\";"),
        "Schedule branch should be preserved. Content:\n{}",
        content
    );
}

#[test]
#[ntest::timeout(1000)]
fn test_union_fragment_codegen_preserves_nested_inline_fragment_type_conditions() {
    let content = run_codegen_fixture(
        "graphox_nested_inline_fragment_key_test",
        r#"
interface Owner {
  id: ID!
  name: String!
}

type User implements Owner {
  id: ID!
  name: String!
  username: String!
}

type Page implements Owner {
  id: ID!
  name: String!
  handle: String!
}

union SearchResult = Album | Playlist

type Album {
  owner: Owner!
}

type Playlist {
  owner: Owner!
}

type Query {
  result: SearchResult
}
"#,
        r#"
const q = gql`
  query TestQuery {
    result {
      ... on Album {
        owner {
          ... on User {
            name
          }
        }
      }
      ... on Playlist {
        owner {
          ... on Page {
            name
          }
        }
      }
    }
  }
`;
"#,
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
    output_dir: "."
    codegen:
      merge_union_types: true
"#,
    );

    assert!(
        !content.contains("__typename: \"Album\" | \"Playlist\""),
        "Members with different nested inline-fragment constraints should not be grouped. Content:\n{}",
        content
    );
    assert!(
        content.contains("__typename: \"Album\";"),
        "Album branch should be preserved. Content:\n{}",
        content
    );
    assert!(
        content.contains("__typename: \"Playlist\";"),
        "Playlist branch should be preserved. Content:\n{}",
        content
    );
}
