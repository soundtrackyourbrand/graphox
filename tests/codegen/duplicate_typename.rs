use std::process::Command;

#[test]
fn test_duplicate_typename_in_unions() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_duplicate_typename_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
union Origin = ManuallyQueuedOrigin | SmartShuffleOrigin

type ManuallyQueuedOrigin {
  id: ID!
}

type SmartShuffleOrigin {
  id: ID!
}

type CatalogItem {
  origin: Origin
}

type Query {
  item: CatalogItem
}
"#,
    )
    .unwrap();

    // Create a fragment file
    // Bug reproduction: Put __typename ONLY in the inline fragment, not in the parent selection set.
    let query_file = temp_dir.join("fragment.ts");
    std::fs::write(
        &query_file,
        r#"
const fragment = gql`
  fragment CatalogItemFragment on CatalogItem {
    origin {
      ... on ManuallyQueuedOrigin {
        __typename
        id
      }
      ... on SmartShuffleOrigin {
        __typename
        id
      }
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
    include: "fragment.ts"
    output_dir: "."
    codegen:
      merge_union_types: true
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    if !output.status.success() {
        panic!(
            "Codegen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let gen_file = temp_dir.join("fragment.codegen.ts");
    let content = std::fs::read_to_string(gen_file).unwrap();

    // Check for grouping of types with identical selection sets
    assert!(
        content.contains(r#"__typename: "ManuallyQueuedOrigin" | "SmartShuffleOrigin";"#),
        "Types with identical selections should be grouped. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
