use std::process::Command;

#[test]
#[ntest::timeout(1000)]
fn test_fragment_import_from_other_file() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_fragment_import_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    // Create schema
    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
type User {
  id: ID!
  name: String!
  profile: Profile
}

type Profile {
  bio: String
  age: Int
}

type Query {
  me: User
}
"#,
    )
    .unwrap();

    // Create a fragment file
    let fragment_file = temp_dir.join("fragments.ts");
    std::fs::write(
        &fragment_file,
        r#"
const f = gql`
  fragment UserInfo on User {
    name
  }
`;
"#,
    )
    .unwrap();

    // Create a query file that uses the fragment inside an inline fragment
    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        r#"
const q = gql`
  query GetMe {
    me {
      ... on User {
        ...UserInfo
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
    include: ["query.ts", "fragments.ts"]
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

    println!("Generated content:\n{}", content);

    // Check if UserInfo is imported
    assert!(
        content.contains("import type { UserInfo } from \"./fragments.codegen\";"),
        "Should import UserInfo from fragments.ts. Content:\n{}",
        content
    );

    // Cleanup
    std::fs::remove_dir_all(temp_dir).ok();
}
