use ahash::AHashMap;
use apollo_compiler::Schema;
use graphql_rust::features::diagnostics::DocumentDiagnostics;
use graphql_rust::{
    Config,
    config::{RequiredFieldRule, RulesConfig},
};
use tower_lsp::lsp_types::DiagnosticSeverity;

use crate::support::{
    assert_diag_range_equals, assert_diagnostic_severity, assert_diagnostic_with_message,
    assert_diagnostics_count, assert_no_diagnostics, create_doc, fixtures,
};

#[test]
#[ntest::timeout(100)]
fn test_required_field_always_true() {
    // Given: a query that selects the `users` field
    let text = r#"
        query GetUsers {
            users {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (always true)
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

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
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(&diagnostics, "Required field 'users'");
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetPosts"));
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
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(false));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );
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
    let mut required_fields = AHashMap::default();
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

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(&diagnostics, "Required field 'users'");
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetPosts"));
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_specific_operation_mutation_not_required() {
    // Use a schema that contains both Query.users and Mutation.createUser
    let schema = fixtures::user_subscription_schema()
        .clone()
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
    let mut required_fields = AHashMap::default();
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
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(100)]
fn test_no_required_fields_config() {
    let text = r#"
        query GetUsers {
            users {
                id
                username
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with NO required field rule
    let config = Config {
        rules: Some(RulesConfig {
            required_fields: None,
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );
    // Expect no diagnostics
    assert_no_diagnostics(&diagnostics);
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

    // Create config with required field rule (with different case)
    let mut required_fields = AHashMap::default();
    required_fields.insert("USERS".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(&diagnostics, "Required field 'USERS'");
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetPosts"));
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
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::Always(true));
    required_fields.insert("posts".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    // We expect 1 diagnostic because 'users' is selected but 'posts' is missing
    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(&diagnostics, "Required field 'posts'");
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetUsers"));
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
    let mut required_fields = AHashMap::default();
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
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_multiple_operations_missing() {
    // Use a schema that contains Mutation.createUser
    let schema = fixtures::user_subscription_schema()
        .clone()
        .validate()
        .unwrap();

    let text = r#"
        mutation CreateUser {
            createUser(username: "test") {
                id
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (only for mutation operations)
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "username".to_string(),
        RequiredFieldRule::Operations(vec!["mutation".to_string()]),
    );

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(&diagnostics, "Required field 'username'");
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "CreateUser"),
    );
}
