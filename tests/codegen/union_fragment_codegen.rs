use std::process::Command;

#[test]
#[ntest::timeout(1000)]
fn test_union_fragment_codegen() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_union_fragment_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
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
    )
    .unwrap();

    // Create a query file
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
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
    )
    .unwrap();

    // Create config
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "query.ts"
    output_dir: "."
"#,
    )
    .unwrap();

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

    let gen_file = temp_dir.join("query.codegen.ts");
    let content = std::fs::read_to_string(gen_file).unwrap();

    println!(
        "Generated content:
{}",
        content
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

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
