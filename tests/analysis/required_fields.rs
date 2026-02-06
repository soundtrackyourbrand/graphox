use apollo_compiler::Schema;
use fnv::FnvHashMap;
use graphql_rust::{
    Config, DocumentState,
    config::{RequiredFieldRule, RulesConfig},
};
use std::sync::OnceLock;
use tower_lsp::lsp_types::*;

// Shared schema for tests
static SCHEMA: OnceLock<Schema> = OnceLock::new();
static VALID_SCHEMA: OnceLock<apollo_compiler::validation::Valid<Schema>> = OnceLock::new();

fn get_schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql")
            .expect("Failed to read schema file");
        Schema::parse(&schema_content, "schema.graphql").expect("Failed to parse schema")
    })
}

fn get_valid_schema() -> &'static apollo_compiler::validation::Valid<Schema> {
    VALID_SCHEMA.get_or_init(|| {
        get_schema()
            .clone()
            .validate()
            .expect("Schema validation failed")
    })
}

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_always_true() {
    let text = r#"
        query GetUser {
            users {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (always true)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have no errors because 'users' is selected
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert!(
        required_field_errors.is_empty(),
        "Expected no required field errors, got: {:?}",
        required_field_errors
    );
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_missing_always_true() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (always true)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have an error because 'users' is not selected
    let required_field_error = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())));

    assert!(
        required_field_error.is_some(),
        "Expected required field error"
    );
    let d = required_field_error.unwrap();
    assert_eq!(
        d.message,
        "Required field 'users' must be selected in query operations"
    );
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_always_false() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (always false, disabled)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(false));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have no errors because rule is disabled
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert!(
        required_field_errors.is_empty(),
        "Expected no required field errors when rule is disabled"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_specific_operation_query() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (only for query operations)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert(
        "users".to_string(),
        RequiredFieldRule::Operations(vec!["query".to_string()]),
    );

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have an error because 'users' is not selected in a query
    let required_field_error = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())));

    assert!(
        required_field_error.is_some(),
        "Expected required field error for query operation"
    );
    let d = required_field_error.unwrap();
    assert_eq!(
        d.message,
        "Required field 'users' must be selected in query operations"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_specific_operation_mutation_not_required() {
    let schema_content = r#"
        type Query {
          users: [User]
        }
        type Mutation {
          createUser(username: String!): User
        }
        type User {
          id: ID!
          username: String!
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        mutation CreateUser {
            createUser(username: "test") {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (only for query operations, not mutations)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert(
        "users".to_string(),
        RequiredFieldRule::Operations(vec!["query".to_string()]),
    );

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should have no errors because rule only applies to queries, not mutations
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert!(
        required_field_errors.is_empty(),
        "Expected no required field errors for mutation when rule only applies to queries"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_multiple_operations() {
    let schema_content = r#"
        type Query {
          users: [User]
        }
        type Mutation {
          createUser(username: String!): User
        }
        type User {
          id: ID!
          username: String!
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        mutation CreateUser {
            createUser(username: "test") {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (for both query and mutation)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert(
        "createUser".to_string(),
        RequiredFieldRule::Operations(vec!["query".to_string(), "mutation".to_string()]),
    );

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should have no errors because 'createUser' is selected in a mutation
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert!(
        required_field_errors.is_empty(),
        "Expected no required field errors, got: {:?}",
        required_field_errors
    );
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_case_insensitive() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (using uppercase QUERY)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert(
        "users".to_string(),
        RequiredFieldRule::Operations(vec!["QUERY".to_string()]),
    );

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have an error because operation type comparison is case-insensitive
    let required_field_error = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())));

    assert!(
        required_field_error.is_some(),
        "Expected required field error (case-insensitive)"
    );
    let d = required_field_error.unwrap();
    assert_eq!(
        d.message,
        "Required field 'users' must be selected in query operations"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_multiple_required_fields() {
    let text = r#"
        query GetUsers {
            users {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with multiple required fields
    let mut required_fields = FnvHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(true));
    required_fields.insert("posts".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have one error for missing 'posts'
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert_eq!(
        required_field_errors.len(),
        1,
        "Expected exactly one required field error"
    );
    assert_eq!(
        required_field_errors[0].message,
        "Required field 'posts' must be selected in query operations"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_subscription() {
    let schema_content = r#"
        type Query {
          users: [User]
        }
        type Subscription {
          userAdded: User
        }
        type User {
          id: ID!
          username: String!
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        subscription {
            userAdded {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (only for subscriptions)
    let mut required_fields = FnvHashMap::default();
    required_fields.insert(
        "userAdded".to_string(),
        RequiredFieldRule::Operations(vec!["subscription".to_string()]),
    );

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should have no errors because 'userAdded' is selected in a subscription
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert!(
        required_field_errors.is_empty(),
        "Expected no required field errors, got: {:?}",
        required_field_errors
    );
}

#[test]
#[ntest::timeout(100)]
fn test_no_required_fields_config() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Config with no rules
    let config = Config {
        rules: None,
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);

    // Should have no required field errors
    let required_field_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("required_field_missing".to_string())))
        .collect();

    assert!(
        required_field_errors.is_empty(),
        "Expected no required field errors when rules config is absent"
    );
}
