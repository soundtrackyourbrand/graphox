use graphox::features::diagnostics::DocumentDiagnostics;
use std::sync::Arc;
use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString};

use crate::support::assert_diag_range_equals;
use crate::support::assert_diagnostic_severity;
use crate::support::assert_diagnostic_with_message;
use crate::support::assert_diagnostics_count;
use crate::support::builders::FragmentInfoBuilder;
use crate::support::create_doc;
use crate::support::fixtures;
use crate::support::range_for_token_at_index;

#[test]
#[ntest::timeout(100)]
fn test_validation_valid_query() {
    let doc = create_doc(
        "file:///valid.graphql",
        r#"
            query GetUser {
                user {
                    id
                    name
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_missing_field() {
    let doc = create_doc(
        "file:///missing.graphql",
        r#"
            query GetUser {
                user {
                    id
                    nonExistentField
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let error = assert_diagnostic_with_message(&diagnostics, "not found on type");
    assert_diagnostic_severity(error, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(
        error,
        &crate::support::range_for_token(
            &doc,
            r#"
            query GetUser {
                user {
                    id
                    nonExistentField
                }
            }
        "#,
            "nonExistentField",
        ),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_deprecated_field() {
    let doc = create_doc(
        "file:///deprecated.graphql",
        r#"
            query GetUser {
                user {
                    id
                    oldField
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_deprecated_field_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let warning = assert_diagnostic_with_message(&diagnostics, "deprecated");
    assert_diagnostic_severity(warning, DiagnosticSeverity::WARNING);
    assert_diag_range_equals(
        warning,
        &crate::support::range_for_token(
            &doc,
            r#"
            query GetUser {
                user {
                    id
                    oldField
                }
            }
        "#,
            "oldField",
        ),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_deprecated_field_ignored() {
    let doc = create_doc(
        "file:///deprecated_ignored.graphql",
        r#"
            query GetUser {
                user {
                    id
                    oldField # graphox-ignore
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_deprecated_field_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 0);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_nested_missing_field() {
    let doc = create_doc(
        "file:///nested.graphql",
        r#"
            query GetPosts {
                posts {
                    id
                    author {
                        username
                        missingInAuthor
                    }
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let error = assert_diagnostic_with_message(&diagnostics, "not found on type");
    assert_diag_range_equals(
        error,
        &crate::support::range_for_token(
            &doc,
            r#"
            query GetPosts {
                posts {
                    id
                    author {
                        username
                        missingInAuthor
                    }
                }
            }
        "#,
            "missingInAuthor",
        ),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_fragment() {
    let doc = create_doc(
        "file:///fragment.graphql",
        r#"
            fragment UserFrag on User {
                id
                missingField
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let error = assert_diagnostic_with_message(&diagnostics, "not found on type");
    assert_diag_range_equals(
        error,
        &crate::support::range_for_token(
            &doc,
            r#"
            fragment UserFrag on User {
                id
                missingField
            }
        "#,
            "missingField",
        ),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_inline_fragment() {
    let text = r#"
        query {
            user {
                ... on User {
                    nonExistentOnUser
                }
            }
        }
    "#;
    let doc = create_doc("file:///inline.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let error = assert_diagnostic_with_message(&diagnostics, "not found on type");
    assert_diag_range_equals(
        error,
        &crate::support::range_for_token(&doc, text, "nonExistentOnUser"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_unknown_fragment_spread() {
    let doc = create_doc(
        "file:///spread.graphql",
        "query { user { ...UnknownFrag } }",
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let error = assert_diagnostic_with_message(&diagnostics, "Unknown fragment");
    assert_diag_range_equals(
        error,
        &crate::support::range_for_token(&doc, "query { user { ...UnknownFrag } }", "UnknownFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_known_fragment_spread() {
    let doc = create_doc(
        "file:///known_spread.graphql",
        r#"query { user { ...KnownFrag } }"#,
    );
    let fragments = vec![FragmentInfoBuilder::new("KnownFrag", "User").build()];
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &fragments,
        None,
        None,
        false,
        true,
    );

    assert!(
        diagnostics.is_empty(),
        "Expected no error for known fragment spread, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_type_only_fragment_unused() {
    let schema = fixtures::user_schema().clone();
    let validated_schema = schema.validate().unwrap();

    let doc = create_doc(
        "file:///test.graphql",
        "fragment UserFrag on User @type_only { id }",
    );

    let used_fragments = ahash::AHashSet::default();
    let diagnostics = doc.get_semantic_diagnostics(
        &validated_schema,
        &[],
        Some(&used_fragments),
        None,
        false,
        true,
    );

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics for @type_only unused fragment, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_type_only_fragment_used() {
    let schema = fixtures::user_schema().clone();
    let validated_schema = schema.validate().unwrap();

    let doc_text = r#"fragment UserFrag on User @type_only { id }

        query { user { ...UserFrag } }"#;
    let doc = create_doc("file:///test.graphql", doc_text);

    let mut used_fragments: ahash::AHashSet<Arc<str>> = ahash::AHashSet::default();
    used_fragments.insert("UserFrag".into());

    let diagnostics = doc.get_semantic_diagnostics(
        &validated_schema,
        &[],
        Some(&used_fragments),
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 2);
    let definition = &diagnostics[0];
    let usage = &diagnostics[1];
    assert_eq!(
        definition.code,
        Some(NumberOrString::String("type_only_used".to_string()))
    );
    assert_eq!(
        definition.message,
        "Fragment 'UserFrag' is used but marked with @type_only. Remove @type_only to resolve this warning."
    );
    assert_diag_range_equals(
        definition,
        &crate::support::range_for_token(&doc, doc_text, "@type_only"),
    );
    assert_diag_range_equals(
        usage,
        &crate::support::range_for_token(&doc, doc_text, "UserFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_input_field_deprecation() {
    let doc_text = r#"
        query {
            test(input: { username: "test", oldField: "value" })
        }
    "#;
    let doc = create_doc("file:///input_deprecated.graphql", doc_text);
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::input_with_deprecated_field_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let warning = assert_diagnostic_with_message(&diagnostics, "deprecated");
    assert_diagnostic_severity(warning, DiagnosticSeverity::WARNING);
    assert_diag_range_equals(
        warning,
        &crate::support::range_for_token(&doc, doc_text, "oldField"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_unions_and_interfaces() {
    let schema = fixtures::union_interface_schema().clone();
    let validated_schema = schema.validate().unwrap();

    let doc_valid = create_doc(
        "file:///valid_union.graphql",
        r#"
            query {
                search(term: "test") {
                    ... on User {
                        id
                        name
                    }
                }
            }
        "#,
    );
    let diagnostics =
        doc_valid.get_semantic_diagnostics(&validated_schema, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Valid union query failed: {:?}",
        diagnostics
    );

    let doc_invalid_text = r#"
        query {
            search(term: "test") {
                ... on User {
                    id
                }
                ... on Bot {
                    message
                    id
                }
            }
        }
    "#;
    let doc_invalid = create_doc("file:///invalid_union.graphql", doc_invalid_text);
    let diagnostics =
        doc_invalid.get_semantic_diagnostics(&validated_schema, &[], None, None, false, true);
    assert_diagnostics_count(&diagnostics, 1);
    let error = assert_diagnostic_with_message(&diagnostics, "not found on type");
    assert_diag_range_equals(
        error,
        &crate::support::range_for_token(&doc_invalid, doc_invalid_text, "message"),
    );

    let doc_interface = create_doc(
        "file:///valid_interface.graphql",
        r#"
            query {
                search(term: "test") {
                    ... on User {
                        id
                        name
                    }
                }
            }
        "#,
    );
    let diagnostics =
        doc_interface.get_semantic_diagnostics(&validated_schema, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Valid interface query failed: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_alias_field_lookup() {
    let doc = create_doc(
        "file:///alias.graphql",
        r#"
            query {
                user {
                    myAlias: id
                    name
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics for aliased field lookup, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_block_strings_and_comments() {
    let doc = create_doc(
        "file:///quirks.graphql",
        r#"
            query GetUser($id: ID! = """123""") { # This is a comment
                playlist(id: $id) {
                    id
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::playlist_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    assert!(
        diagnostics.is_empty(),
        "Block strings or comments caused issues: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_circular_fragments() {
    let doc = create_doc(
        "file:///circular.graphql",
        r#"
            fragment UserFrag on User {
                id
                ...SharedFields
            }

            fragment SharedFields on User {
                name
                ...UserFrag
            }

            query {
                user {
                    ...UserFrag
                }
            }
        "#,
    );
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    let circular_count = diagnostics
        .iter()
        .filter(|d| d.message.contains("circular"))
        .count();
    assert!(
        circular_count >= 2,
        "Expected at least 2 circular diagnostics, got {}",
        circular_count
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_circular_fragments_three_way() {
    let text = r#"
            fragment A on User {
                id
                ...B
            }

            fragment B on User {
                name
                ...C
            }

            fragment C on User {
                name
                ...A
            }

            query {
                user {
                    ...A
                }
            }
        "#;
    let doc = create_doc("file:///circular3.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_schema().clone().validate().unwrap(),
        &[],
        None,
        None,
        false,
        true,
    );

    // Diagnostic: A -> B (on line 3)
    assert!(
        diagnostics
            .iter()
            .any(|d| d.range == range_for_token_at_index(&doc, text, "B", 0))
    );
    // Diagnostic: B -> C (on line 8)
    assert!(
        diagnostics
            .iter()
            .any(|d| d.range == range_for_token_at_index(&doc, text, "C", 0))
    );
    // Diagnostic: C -> A (on line 13)
    assert!(
        diagnostics
            .iter()
            .any(|d| d.range == range_for_token_at_index(&doc, text, "A", 1))
    );
    println!("{:#?}", diagnostics);
    let circular_count = diagnostics
        .iter()
        .filter(|d| d.message.contains("circular"))
        .count();
    assert!(
        circular_count == 3,
        "Expected 3 circular diagnostics, got {}",
        circular_count
    );
}
