use graphox::Config;
use graphox::config::RulesConfig;
use graphox::features::diagnostics::DocumentDiagnostics;

use crate::support::{
    assert_diagnostic_with_message, assert_diagnostics_count, create_doc, fixtures,
};

#[test]
#[ntest::timeout(100)]
fn test_required_fields_simple() {
    let schema = fixtures::user_with_posts_schema()
        .clone()
        .validate()
        .unwrap();
    let query = "query { users { id } }";
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(100)]
fn test_required_fields_nested() {
    let schema = fixtures::user_with_posts_schema()
        .clone()
        .validate()
        .unwrap();
    let query = "query { posts { author { id } } }";
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(100)]
fn test_required_fields_fragment_spread() {
    let schema = fixtures::user_with_posts_schema()
        .clone()
        .validate()
        .unwrap();
    let query = r#"
        query {
            users {
                ...UserFields
            }
        }
        fragment UserFields on User {
            id
        }
    "#;
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(100)]
fn test_required_fields_inline_fragment() {
    let schema = fixtures::union_interface_schema()
        .clone()
        .validate()
        .unwrap();
    let query = r#"
        query {
            search(term: "test") {
                ... on User {
                    id
                    name
                }
                ... on Bot {
                    id
                    name
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(100)]
fn test_no_duplicate_fields_object() {
    let schema = fixtures::user_with_posts_schema()
        .clone()
        .validate()
        .unwrap();
    let query = "query { users { id id } }";
    let doc = create_doc("file:///test.graphql", query);
    let config = Config {
        rules: Some(RulesConfig {
            no_duplicate_fields: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
    assert_diagnostics_count(&diags, 1);
    assert_diagnostic_with_message(&diags, "Duplicate field 'id'");
}

#[test]
#[ntest::timeout(100)]
fn test_no_duplicate_fields_args() {
    let schema = fixtures::user_with_posts_schema()
        .clone()
        .validate()
        .unwrap();
    let query = "query { posts { title title } }";
    let doc = create_doc("file:///test.graphql", query);
    let config = Config {
        rules: Some(RulesConfig {
            no_duplicate_fields: Some(true),
            ..RulesConfig::default()
        }),
        ..Default::default()
    };
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
    assert_diagnostics_count(&diags, 1);
    assert_diagnostic_with_message(&diags, "Duplicate field 'title'");
}
