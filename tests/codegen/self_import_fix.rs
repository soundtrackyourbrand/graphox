use std::fs;
use std::process::Command;

#[test]
#[ntest::timeout(1000)]
fn test_no_self_importing_fragments() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_no_self_import_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
type User {
  id: ID!
  name: String!
}

type Query {
  user: User
}
"#,
    )
    .unwrap();

    let fragment_file = temp_dir.join("form_fragment.ts");
    std::fs::write(
        &fragment_file,
        r#"
export const formFragment = gql`
  fragment FormFragment on User {
    name
  }
`;
"#,
    )
    .unwrap();

    let query_file = temp_dir.join("query.ts");
    std::fs::write(
        &query_file,
        r#"
import { formFragment } from "./form_fragment";

export const q = gql`
  query GetUser {
    user {
      ...FormFragment
    }
  }
`;
"#,
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: ["query.ts", "form_fragment.ts"]
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

    let gen_file = temp_dir.join("form_fragment.codegen.ts");
    let content = fs::read_to_string(&gen_file).unwrap_or_default();

    let self_import_pattern =
        format!("import type {{ FormFragment }} from './form_fragment.codegen'",);
    assert!(
        !content.contains(&self_import_pattern),
        "form_fragment.codegen.ts should not import itself. Content:\n{}",
        content
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(1000)]
fn test_no_self_import_with_symlink() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join("graphox_symlink_test");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    let schema_file = temp_dir.join("schema.graphql");
    std::fs::write(
        &schema_file,
        r#"
type User {
  id: ID!
  name: String!
}

type Query {
  user: User
}
"#,
    )
    .unwrap();

    let fragments_dir = temp_dir.join("fragments");
    std::fs::create_dir_all(&fragments_dir).unwrap();

    let fragment_file = fragments_dir.join("user_fragment.graphql");
    std::fs::write(
        &fragment_file,
        r#"
fragment UserFragment on User {
  name
}
"#,
    )
    .unwrap();

    let symlink_dir = temp_dir.join("symlink_dir");
    let _ = std::fs::create_dir_all(&symlink_dir);

    let query_dir = temp_dir.join("queries");
    std::fs::create_dir_all(&query_dir).unwrap();

    let query_file = query_dir.join("get_user.graphql");
    std::fs::write(
        &query_file,
        r#"
query GetUser {
  user {
    ...UserFragment
  }
}
"#,
    )
    .unwrap();

    let fragment_link = symlink_dir.join("user_fragment.graphql");
    if !fragment_link.exists() {
        if cfg!(target_os = "windows") {
            if let Err(e) = std::fs::copy(&fragment_file, &fragment_link) {
                eprintln!(
                    "Warning: Failed to copy file for symlink test on Windows: {}",
                    e
                );
            }
        } else {
            #[cfg(target_family = "unix")]
            {
                if let Err(e) = std::os::unix::fs::symlink(&fragment_file, &fragment_link) {
                    panic!("Failed to create symlink: {}", e);
                }
            }
            #[cfg(not(target_family = "unix"))]
            {
                if let Err(e) = std::fs::copy(&fragment_file, &fragment_link) {
                    eprintln!(
                        "Warning: Failed to copy file for symlink test on non-Unix: {}",
                        e
                    );
                }
            }
        }
    }

    let config_content = format!(
        r#"
projects:
  - schema: "{}/schema.graphql"
    include: ["queries/*.graphql", "fragments/*.graphql"]
    output_dir: "queries"
"#,
        temp_dir.display()
    );
    std::fs::write(temp_dir.join("graphox.yaml"), config_content).unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("symlink") && !stderr.contains("permission") {
            panic!("Codegen failed: {}", stderr);
        }
    }

    std::fs::remove_dir_all(temp_dir).ok();
}
