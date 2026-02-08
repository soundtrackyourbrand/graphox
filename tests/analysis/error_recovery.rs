#![allow(unused_imports)]

use crate::support::create_doc;
use crate::support::fixtures::{union_interface_schema, user_schema};
use graphql_rust::features::diagnostics::DocumentDiagnostics;
use tower_lsp::lsp_types::*;

#[test]
#[ntest::timeout(100)]
fn test_malformed_query_partial() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query GetUser {
            user {
                id
                name
            }
            # Missing closing brace is intentional
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Parser should report errors for malformed query"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_missing_closing_braces() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query {
            user {
                id
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    let has_expected_error = diagnostics
        .iter()
        .any(|d| d.message.contains("Expected") || d.message.contains("Syntax Error"));

    assert!(
        has_expected_error,
        "Should have errors about missing closing braces, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_invalid_type_condition() {
    let schema = union_interface_schema().clone().validate().unwrap();

    let text = r#"
        query {
            search(term: "test") {
                ... on NonExistentType {
                    id
                }
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report error for invalid type condition"
    );

    let has_type_error = diagnostics
        .iter()
        .any(|d| d.message.contains("NonExistentType") || d.message.contains("type"));

    assert!(
        has_type_error,
        "Should have error about unknown type, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_unknown_directive_location() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query {
            user {
                id
                name @deprecated(reason: "Use something else")
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report diagnostic for deprecated field usage"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_partial_fragment_definition() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        fragment UserFields on User
        # Fragment definition is incomplete
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report error for incomplete fragment"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_unclosed_string_value() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query {
            user(id: "unclosed string) {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report syntax error for unclosed string"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_invalid_field_selection() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query {
            user {
                id
                (invalid selection)
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report error for invalid field selection"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_missing_query_name() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query {
            user {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(diagnostics.is_empty(), "Anonymous query should be valid");
}

#[test]
#[ntest::timeout(100)]
fn test_malformed_variable_definition() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query GetUser($id: ID! = ) {
            user(id: $id) {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report error for malformed variable default value"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_invalid_argument_syntax() {
    let schema = user_schema().clone().validate().unwrap();

    let text = r#"
        query {
            user(id: @invalidArg) {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Should report error for invalid argument syntax"
    );
}
