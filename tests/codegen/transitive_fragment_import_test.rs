use std::process::Command;

/// Reproduces the bug where a fragment that is only used *transitively* (i.e., spread inside
/// another fragment in a different file) does not get an `import` statement generated in the
/// codegen file that references it by name in its TypeScript output.
///
/// Concrete scenario (mirrors packages/playback/src/base.ts + remote.ts):
///
///   base.ts defines:
///     - `fragment PlaybackDisplay on Display { ... }`
///     - `fragment PlaylistInfo on Playlist { display { ...PlaybackDisplay } }`
///
///   remote.ts defines:
///     - a query that spreads `...PlaylistInfo` (NEVER spreads `...PlaybackDisplay` directly)
///
/// Codegen output in base.codegen.ts:
///   export interface PlaybackDisplayFragment { ... }
///   export interface PlaylistInfoFragment {
///     display: ({ __typename: "Display" } & PlaybackDisplayFragment) | null;
///   }
///
/// Expected codegen in remote.codegen.ts:
///   import type { PlaylistInfoFragment, PlaybackDisplayFragment } from "./base.codegen";
///   // PlaybackDisplayFragment must be imported because it appears by name in the
///   // body of PlaylistInfoFragment, which remote.codegen.ts imports.
///
/// Actual (buggy) codegen in remote.codegen.ts:
///   import type { PlaylistInfoFragment } from "./base.codegen";
///   // PlaybackDisplayFragment is NOT imported even though it's referenced by name
///   // inside PlaylistInfoFragment's type definition — TypeScript cannot resolve it.
#[test]
#[ntest::timeout(15000)]
fn test_transitive_fragment_not_imported_from_other_file() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_transitive_fragment_import_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // --- Schema ---
    std::fs::write(
        temp_dir.join("schema.graphql"),
        r#"
type Query {
  playlist(id: ID!): Playlist
}

type Playlist {
  id: ID!
  name: String!
  display: Display
}

type Track {
  id: ID!
  title: String!
  album: Album
}

type Album {
  id: ID!
  title: String!
  display: Display
}

type Display {
  image: Image
}

type Image {
  placeholder: String
}
"#,
    )
    .unwrap();

    // --- base.ts ---
    // Defines PlaybackDisplay (no @public) and TrackInfo (no @public) which spreads PlaybackDisplay.
    // Also defines PlaylistInfo which spreads PlaybackDisplay.
    // This mirrors base.ts in the playback package.
    std::fs::write(
        temp_dir.join("base.ts"),
        r#"
const a = gql`
  fragment PlaybackDisplay on Display {
    image {
      placeholder
    }
  }
`;

const b = gql`
  fragment TrackInfo on Track {
    id
    title
    album {
      id
      title
      display {
        ...PlaybackDisplay
      }
    }
  }
`;

const c = gql`
  fragment PlaylistInfo on Playlist {
    id
    name
    display {
      ...PlaybackDisplay
    }
  }
`;
"#,
    )
    .unwrap();

    // --- remote.ts ---
    // Directly spreads ...TrackInfo and ...PlaylistInfo in its operations,
    // but never directly spreads ...PlaybackDisplay.
    // This mirrors remote.ts in the playback package.
    std::fs::write(
        temp_dir.join("remote.ts"),
        r#"
const q = gql`
  query GetPlaylist($id: ID!) {
    playlist(id: $id) {
      ...PlaylistInfo
    }
  }
`;
"#,
    )
    .unwrap();

    // --- graphox.yaml ---
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include:
      - "base.ts"
      - "remote.ts"
    output_dir: "."
    codegen:
      fragment_suffix: "Fragment"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute graphox");

    assert!(
        output.status.success(),
        "Codegen failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let remote_codegen = temp_dir.join("remote.codegen.ts");
    let content = std::fs::read_to_string(&remote_codegen)
        .unwrap_or_else(|_| panic!("remote.codegen.ts was not generated"));

    println!("=== remote.codegen.ts ===\n{content}");

    // Sanity check: PlaylistInfoFragment IS imported (directly spread in remote.ts).
    assert!(
        content.contains("PlaylistInfoFragment") && content.contains("\"./base.codegen\""),
        "PlaylistInfoFragment should be imported (it is directly spread in remote.ts).\nContent:\n{content}"
    );

    // BUG ASSERTION: PlaybackDisplayFragment is referenced BY NAME inside the body of
    // PlaylistInfoFragment in base.codegen.ts
    // Confirm the bug is fixed: PlaybackDisplayFragment IS referenced AND imported.
    assert!(
        content.contains("PlaybackDisplayFragment"),
        "Expected PlaybackDisplayFragment to be referenced in remote.codegen.ts (via PlaylistInfoFragment).\nContent:\n{content}"
    );

    assert!(
        content.contains("PlaybackDisplayFragment") && content.contains("\"./base.codegen\""),
        "PlaybackDisplayFragment should be imported from base.codegen.\nContent:\n{content}"
    );

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).ok();
}
