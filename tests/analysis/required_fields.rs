use apollo_compiler::Schema;
use fnv::FnvHashMap;
use graphql_rust::{
    Config,
    config::{RequiredFieldRule, RulesConfig},
};
use tower_lsp::lsp_types::DiagnosticSeverity;

use crate::support::{assert_no_diagnostics, create_doc, get_valid_schema};

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
    
    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.message, "Required field 'users' must be selected in query operations");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    crate::support::assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetPosts"));
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
    
    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.message, "Required field 'users' must be selected in query operations");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    crate::support::assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetPosts"));
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
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(100)]
fn test_no_required_fields_config() {
    let text = r#"
        query GetUser {
            users {
                id
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

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);
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
    let mut required_fields = FnvHashMap::default();
    required_fields.insert("USERS".to_string(), RequiredFieldRule::Always(true));

    let config = Config {
        rules: Some(RulesConfig {
            required_fields: Some(required_fields),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };

    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, Some(&config), false, true);
    
    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.message, "Required field 'USERS' must be selected in query operations");
    crate::support::assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetPosts"));
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
    
    // We expect 1 diagnostic because 'users' is selected but 'posts' is missing
    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.message, "Required field 'posts' must be selected in query operations");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    crate::support::assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "GetUsers"));
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
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(100)]
fn test_required_field_multiple_operations_missing() {
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
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (only for mutation operations)
    let mut required_fields = FnvHashMap::default();
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

    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.message, "Required field 'username' must be selected in mutation operations");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    crate::support::assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "CreateUser"));
}
