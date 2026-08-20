use std::process::Command;

/// Reproduces the bug where a fragment that is only used *transitively* (i.e., spread inside
/// another fragment in a different file) does not get an `import` statement generated in the
/// codegen file that references it by name in its TypeScript output.
///
/// Concrete scenario (mirrors packages/catalog/src/base.ts + remote.ts):
///
///   base.ts defines:
///     - `fragment ProductCard on Product { ... }`
///     - `fragment PlaylistInfo on Playlist { product { ...ProductCard } }`
///
///   remote.ts defines:
///     - a query that spreads `...PlaylistInfo` (NEVER spreads `...ProductCard` directly)
///
/// Codegen output in catalog.codegen.ts:
///   export interface ProductCardFragment { ... }
///   export interface PlaylistInfoFragment {
///     product: ({ __typename: "Product" } & ProductCardFragment) | null;
///   }
///
/// Expected codegen in checkout.codegen.ts:
///   import type { PlaylistInfoFragment, ProductCardFragment } from "./catalog.codegen";
///   // ProductCardFragment must be imported because it appears by name in the
///   // body of PlaylistInfoFragment, which checkout.codegen.ts imports.
///
/// Actual (buggy) codegen in checkout.codegen.ts:
///   import type { PlaylistInfoFragment } from "./catalog.codegen";
///   // ProductCardFragment is NOT imported even though it's referenced by name
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
  product: Product
}

type Track {
  id: ID!
  title: String!
  album: Album
}

type Album {
  id: ID!
  title: String!
  product: Product
}

type Product {
  image: Image
}

type Image {
  placeholder: String
}
"#,
    )
    .unwrap();

    // --- base.ts ---
    // Defines ProductCard (no @public) and TrackInfo (no @public) which spreads ProductCard.
    // Also defines PlaylistInfo which spreads ProductCard.
    // This mirrors base.ts in the catalog package.
    std::fs::write(
        temp_dir.join("catalog.ts"),
        r#"
const a = gql`
  fragment ProductCard on Product {
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
      product {
        ...ProductCard
      }
    }
  }
`;

const c = gql`
  fragment PlaylistInfo on Playlist {
    id
    name
    product {
      ...ProductCard
    }
  }
`;
"#,
    )
    .unwrap();

    // --- remote.ts ---
    // Directly spreads ...TrackInfo and ...PlaylistInfo in its operations,
    // but never directly spreads ...ProductCard.
    // This mirrors remote.ts in the catalog package.
    std::fs::write(
        temp_dir.join("checkout.ts"),
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
      - "catalog.ts"
      - "checkout.ts"
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

    let checkout_codegen = temp_dir.join("checkout.codegen.ts");
    let content = std::fs::read_to_string(&checkout_codegen)
        .unwrap_or_else(|_| panic!("checkout.codegen.ts was not generated"));

    println!("=== checkout.codegen.ts ===\n{content}");

    // Sanity check: PlaylistInfoFragment IS imported (directly spread in remote.ts).
    assert!(
        content.contains("PlaylistInfoFragment") && content.contains("\"./catalog.codegen\""),
        "PlaylistInfoFragment should be imported (it is directly spread in remote.ts).\nContent:\n{content}"
    );

    // BUG ASSERTION: ProductCardFragment is referenced BY NAME inside the body of
    // PlaylistInfoFragment in catalog.codegen.ts
    // Confirm the bug is fixed: ProductCardFragment IS referenced AND imported.
    assert!(
        content.contains("ProductCardFragment"),
        "Expected ProductCardFragment to be referenced in checkout.codegen.ts (via PlaylistInfoFragment).\nContent:\n{content}"
    );

    assert!(
        content.contains("ProductCardFragment") && content.contains("\"./catalog.codegen\""),
        "ProductCardFragment should be imported from catalog.codegen.\nContent:\n{content}"
    );

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).ok();
}
