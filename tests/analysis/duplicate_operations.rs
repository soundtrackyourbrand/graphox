use crate::support::{create_doc, range_for_token_at_index};
use apollo_compiler::Schema;
use graphql_rust::{
    Config,
    config::{GlobPattern, ProjectConfig, RulesConfig, SchemaSource},
};
use tempfile::tempdir;
use tower_lsp::lsp_types::{NumberOrString, Url};

fn get_schema() -> apollo_compiler::validation::Valid<Schema> {
    let schema_text = r#"
        type Query {
            user(id: ID!): User
        }
        
        type User {
            id: ID!
            name: String!
        }
    "#;
    Schema::parse(schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap()
}

#[test]
#[ntest::timeout(5000)]
fn test_duplicate_operation_names_same_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser($id: ID) {
            user(id: $id) {
                id
                name
            }
        }
        
        query GetUser($id: ID) {
            user(id: $id) {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = Url::from_file_path(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config {
        base_dir: dir.path().to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Only expect our internal diagnostic now (reported once per name per file)
    assert_eq!(diagnostics.len(), 1);
    
    let d = &diagnostics[0];
    assert_eq!(d.message, "Duplicate operation name 'GetUser'");
    // First GetUser name
    crate::support::assert_diag_range_equals(
        d,
        &range_for_token_at_index(&doc, content, "GetUser", 0),
    );
}

#[test]
#[ntest::timeout(5000)]
fn test_unique_operation_names_no_error() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser {
            user(id: "1") {
                id
                name
            }
        }
        
        query GetAllUsers {
            user(id: "2") {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = Url::from_file_path(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config {
        base_dir: dir.path().to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should NOT have diagnostics for duplicate operations
    // Check there are no duplicate_operation diagnostics
    let duplicate_errors: Vec<_> = diagnostics
        .iter()
        .filter(
            |d| matches!(d.code, Some(NumberOrString::String(ref s)) if s == "duplicate_operation"),
        )
        .collect();

    assert!(
        duplicate_errors.is_empty(),
        "Should not detect duplicates when names are unique"
    );
}

#[test]
#[ntest::timeout(5000)]
fn test_duplicate_operation_rule_disabled() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser {
            user(id: "1") {
                id
                name
            }
        }
        
        query GetUser {
            user(id: "2") {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = Url::from_file_path(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config {
        base_dir: dir.path().to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(false),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should NOT have diagnostics when rule is disabled
    let duplicate_errors: Vec<_> = diagnostics
        .iter()
        .filter(
            |d| matches!(d.code, Some(NumberOrString::String(ref s)) if s == "duplicate_operation"),
        )
        .collect();

    assert!(
        duplicate_errors.is_empty(),
        "Should not detect duplicates when rule is disabled"
    );
}

#[test]
#[ntest::timeout(5000)]
fn test_duplicate_operation_no_rules_config() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser {
            user(id: "1") {
                id
                name
            }
        }
        
        query GetUser {
            user(id: "2") {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = Url::from_file_path(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config {
        base_dir: dir.path().to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: None,
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should NOT have diagnostics when no rules config exists
    let duplicate_errors: Vec<_> = diagnostics
        .iter()
        .filter(
            |d| matches!(d.code, Some(NumberOrString::String(ref s)) if s == "duplicate_operation"),
        )
        .collect();

    assert!(
        duplicate_errors.is_empty(),
        "Should not detect duplicates when rules config is not provided"
    );
}
