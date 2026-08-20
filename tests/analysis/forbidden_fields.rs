use ahash::AHashMap;
use graphox::Config;
use graphox::config::{ForbiddenFieldRule, RulesConfig};
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp::lsp_types::*;

use crate::support::{
    assert_diag_range_equals, assert_diagnostic_severity, assert_diagnostic_with_message,
    assert_diagnostics_count, assert_no_diagnostics, create_doc, fixtures,
};

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_always_true() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "password"));
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_always_false() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_always(false),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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
#[ntest::timeout(300)]
fn test_forbidden_field_specific_operation_mutation() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Forbidden ONLY in mutations
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["mutation".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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

    // Should NOT error in a query
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_ignored_with_inline_comment() {
    let text = r#"
        query GetUsers {
            users { # graphox-ignore
                id
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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
#[ntest::timeout(300)]
fn test_forbidden_field_with_reason() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_always(true)
            .with_reason("Passwords must not be fetched directly".to_string()),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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
    assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User' in query operations: Passwords must not be fetched directly",
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_type_specific() {
    let text = r#"
        query GetData {
            users {
                name
            }
            posts {
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let yaml = r#"
      forbidden_fields:
        User:
          name: true
    "#;
    let yaml_docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    let rules = RulesConfig::from_yaml(&yaml_docs[0]).unwrap();

    let config = Config::default().with_rules(rules);

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

    // Should error on User.name but NOT on Post.title (even if both were named 'name')
    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(&diagnostics, "Field 'name' is forbidden on type 'User'");
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_type_override() {
    let text = r#"
        query GetUsers {
            users {
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Global forbidden, but allowed on User
    let yaml = r#"
      forbidden_fields:
        password: true
        User:
          password: false
    "#;
    let yaml_docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    let rules = RulesConfig::from_yaml(&yaml_docs[0]).unwrap();

    let config = Config::default().with_rules(rules);

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
#[ntest::timeout(300)]
fn test_forbidden_field_same_response_key_different_types() {
    // The response key `subscription` is reached via two paths that resolve to
    // different types. A type-specific forbidden rule on `ZoneSubscription`
    // resolves against that type, independent of the `AccountSubscription`
    // reached via the other path that shares the leaf response key.
    let text = r#"
        query Combo {
            soundZone {
                id
                subscription {
                    state
                }
            }
            account {
                id
                billing {
                    subscription {
                        billingCycle
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let yaml = r#"
      forbidden_fields:
        ZoneSubscription:
          state: true
    "#;
    let yaml_docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    let rules = RulesConfig::from_yaml(&yaml_docs[0]).unwrap();

    let config = Config::default().with_rules(rules);

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::colliding_response_key_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    // Exactly one diagnostic: `state` forbidden on ZoneSubscription, anchored at
    // the `state` selection under the zone subscription (not the account path,
    // which selects `billingCycle` and carries no forbidden rule).
    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'state' is forbidden on type 'ZoneSubscription'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "state"));
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_nested_inside_fragment_definition() {
    // Same gap as `required_fields`: a nested selection inside a fragment body
    // must be checked, and the diagnostic lands on the fragment's own selection.
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author {
                id
                password
            }
        }

        query GetPosts {
            posts {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "password"));
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_operation_scoped_rule_skipped_inside_fragment() {
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author {
                id
                password
            }
        }

        query GetPosts {
            posts {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["query".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

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
