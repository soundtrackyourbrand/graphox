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
                username
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::Always(true));

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
    let d = assert_diagnostic_with_message(&diagnostics, "Field 'password' is forbidden");
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
                username
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::Always(false));

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
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Forbidden ONLY in mutations
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::Operations(vec!["mutation".to_string()]),
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
            users {
                id
                password # graphox-ignore
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::Always(true));

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
fn test_forbidden_field_nested() {
    let text = r#"
        query GetPosts {
            posts {
                id
                secretField
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("secretField".to_string(), ForbiddenFieldRule::Always(true));

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
        "Field 'secretField' is forbidden in query operations",
    );
}
