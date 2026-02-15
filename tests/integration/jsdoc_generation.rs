use std::fs;
use std::process::Command;

#[test]
fn test_jsdoc_generation_e2e() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // 1. Create schema
    let schema = r#"
    type Query {
        """
        Get the current user
        """
        me: User
        
        deprecatedField: String @deprecated(reason: "Use me instead")
    }

    type User {
        """
        The user's ID
        """
        id: ID!
        
        """
        The user's name
        """
        name: String!
        
        oldEmail: String @deprecated(reason: "Use email instead")
    }
    "#;
    fs::write(root.join("schema.graphql"), schema).unwrap();

    // 2. Create query
    let query = r#"
    query Me {
        me {
            id
            name
            oldEmail
        }
        deprecatedField
    }
    "#;
    fs::write(root.join("query.graphql"), query).unwrap();

    // 3. Create config
    let config = r#"
projects:
  - schema: schema.graphql
    include: 
      - "**/*.graphql"
    output_dir: "generated"
    codegen:
      re_exports: true
"#;
    fs::write(root.join("graphox.yaml"), config).unwrap();

    // 4. Run codegen
    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(root)
        .output()
        .expect("Failed to run codegen");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 5. Verify output
    let generated_path = root.join("generated/query.codegen.ts");
    assert!(
        generated_path.exists(),
        "Generated file not found at {:?}",
        generated_path
    );

    let content = fs::read_to_string(generated_path).unwrap();

    // Verify JSDoc
    println!("Generated content:\n{}", content);

    // We look for parts of JSDoc
    assert!(
        content.contains("* Get the current user"),
        "Missing query field description"
    );
    assert!(
        content.contains("@deprecated Use me instead"),
        "Missing query field deprecation"
    );

    assert!(
        content.contains("* The user's ID"),
        "Missing User.id description"
    );
    assert!(
        content.contains("* The user's name"),
        "Missing User.name description"
    );
    assert!(
        content.contains("@deprecated Use email instead"),
        "Missing User.oldEmail deprecation"
    );
}

#[test]
fn test_scalar_mapping() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // 1. Create schema with scalars
    let schema = r#"
    scalar DateTime
    scalar CustomID

    type Query {
        time: DateTime
        cid: CustomID
    }
    "#;
    fs::write(root.join("schema.graphql"), schema).unwrap();

    // 2. Create query
    let query = r#"
    query Time {
        time
        cid
    }
    "#;
    fs::write(root.join("query.graphql"), query).unwrap();

    // 3. Create config with mapping for DateTime but not CustomID
    let config = r#"
scalars:
  DateTime: string
projects:
  - schema: schema.graphql
    include: 
      - "**/*.graphql"
    output_dir: "generated"
"#;
    fs::write(root.join("graphox.yaml"), config).unwrap();

    // 4. Run codegen
    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(root)
        .output()
        .expect("Failed to run codegen");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 5. Verify output
    let generated_path = root.join("generated/query.codegen.ts");
    let content = fs::read_to_string(generated_path).unwrap();

    println!("Generated content:\n{}", content);

    // DateTime should be string
    assert!(
        content.contains("time: string | null"),
        "DateTime should be mapped to string"
    );

    // CustomID should be any (default)
    assert!(
        content.contains("cid: any | null"),
        "CustomID should be default to any"
    );
}
