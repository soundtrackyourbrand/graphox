use apollo_compiler::Schema;
use fnv::FnvHashMap;
use graphql_rust::{
    Config,
    config::{RequiredFieldRule, RulesConfig},
};
use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString};

use crate::support::{
    assert_diag_message_equals, assert_no_diagnostics, create_doc, get_valid_schema, range,
};

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
    // Expect no diagnostics
    assert_no_diagnostics(&diagnostics);
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
    let expected_msg = "Required field 'users' must be selected in query operations";
    let d = assert_diag_message_equals(&diagnostics, expected_msg);
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    // Range should point at the query operation name because the field is missing from the root
    let expected_range = range(1, 8, 6, 9);
    crate::support::assert_diag_range_equals(d, &expected_range);
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
    assert_no_diagnostics(&diagnostics);
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
    let expected_msg = "Required field 'users' must be selected in query operations";
    let d = assert_diag_message_equals(&diagnostics, expected_msg);
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    let expected_range = range(1, 8, 6, 9);
    crate::support::assert_diag_range_equals(d, &expected_range);
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
    let expected_msg = "Required field 'posts' must be selected in query operations";
    let d = assert_diag_message_equals(&diagnostics, expected_msg);
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    let expected_range = range(1, 8, 6, 9);
    crate::support::assert_diag_range_equals(d, &expected_range);
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
