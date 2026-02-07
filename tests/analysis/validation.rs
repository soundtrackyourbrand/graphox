use apollo_compiler::Schema;
use graphql_rust::features::completion::FragmentCompletionInfo;
use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString, Url};

use crate::support::{create_doc, get_valid_schema, range};

// Shared schema for tests

#[test]
#[ntest::timeout(100)]
fn test_validation_valid_query() {
    let text = r#"
        query GetUser {
            users {
                id
                username
                email
            }
        }
    "#;
    let doc = create_doc("file:///valid.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_missing_field() {
    let text = r#"
        query GetUser {
            users {
                id
                nonExistentField
            }
        }
    "#;
    let doc = create_doc("file:///missing.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let expected_message = format!(
        "Field '{}' not found on type '{}'",
        "nonExistentField", "User"
    );
    let error = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected 'not found' error '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    assert_eq!(error.severity, Some(DiagnosticSeverity::ERROR));
    // Range should point at the field name occurrence
    let expected = crate::support::range_for_token(&doc, text, "nonExistentField");
    assert_eq!(error.range.start, expected.start);
    assert_eq!(error.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_deprecated_field() {
    let text = r#"
        query GetUser {
            users {
                id
                oldField
            }
        }
    "#;
    let doc = create_doc("file:///deprecated.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let expected_message = "Field 'oldField' is deprecated: Use username instead".to_string();
    let warning = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected 'deprecated' warning '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
    // Range should point at the field name
    let expected = crate::support::range_for_token(&doc, text, "oldField");
    assert_eq!(warning.range.start, expected.start);
    assert_eq!(warning.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_nested_missing_field() {
    let text = r#"
        query GetPosts {
            posts {
                id
                author {
                    username
                    missingInAuthor
                }
            }
        }
    "#;
    let doc = create_doc("file:///nested.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let expected_message = format!(
        "Field '{}' not found on type '{}'",
        "missingInAuthor", "User"
    );
    let error = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected nested missing field error '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    // Range should point at the missing field
    let expected = crate::support::range_for_token(&doc, text, "missingInAuthor");
    assert_eq!(error.range.start, expected.start);
    assert_eq!(error.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_fragment() {
    let text = r#"
        fragment UserFrag on User {
            id
            missingInFragment
        }
    "#;
    let doc = create_doc("file:///fragment.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let expected_message = format!(
        "Field '{}' not found on type '{}'",
        "missingInFragment", "User"
    );
    let error = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected fragment error '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    let expected = crate::support::range_for_token(&doc, text, "missingInFragment");
    assert_eq!(error.range.start, expected.start);
    assert_eq!(error.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_inline_fragment() {
    let text = r#"
        query {
            users {
                ... on User {
                    id
                    nonExistentOnUser
                }
            }
        }
    "#;
    let doc = create_doc("file:///inline.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let expected_message = format!(
        "Field '{}' not found on type '{}'",
        "nonExistentOnUser", "User"
    );
    let error = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected inline fragment error '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    let expected = crate::support::range_for_token(&doc, text, "nonExistentOnUser");
    assert_eq!(error.range.start, expected.start);
    assert_eq!(error.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_unknown_fragment_spread() {
    let text = r#"
        query {
            users {
                ...UnknownFrag
            }
        }
    "#;
    let doc = create_doc("file:///spread.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let expected_message = "Unknown fragment: UnknownFrag".to_string();
    let error = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected unknown fragment error '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    let expected = crate::support::range_for_token(&doc, text, "UnknownFrag");
    assert_eq!(error.range.start, expected.start);
    assert_eq!(error.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_known_fragment_spread() {
    let text = r#"
        query {
            users {
                ...KnownFrag
            }
        }
    "#;
    let doc = create_doc("file:///known_spread.graphql", text);
    let fragments = vec![FragmentCompletionInfo {
        name: "KnownFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::parse("file:///test.graphql").unwrap(),
        package_root: None,
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    }];
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &fragments, None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no error for known fragment spread"
    );
}

#[test]
fn test_type_only_fragment_unused() {
    let schema_content = "type User { id: ID! } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        fragment UserFrag on User @type_only {
            id
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Should NOT have diagnostics for unused fragment
    let used_fragments = fnv::FnvHashSet::default();
    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[], Some(&used_fragments), None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics for @type_only unused fragment, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_type_only_fragment_used() {
    let schema_content = "type User { id: ID! } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        fragment UserFrag on User @type_only {
            id
        }
        
        query {
            me {
                ...UserFrag
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Should HAVE a warning because it's used but marked @type_only
    let mut used_fragments = fnv::FnvHashSet::default();
    used_fragments.insert("UserFrag".to_string());

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[], Some(&used_fragments), None, false, true);

    let warning = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("type_only_used".to_string())));
    assert!(
        warning.is_some(),
        "Expected warning for @type_only fragment being used"
    );
    let w = warning.unwrap();
    assert_eq!(
        w.message,
        "Fragment 'UserFrag' is used but marked with @type_only. Remove @type_only to resolve this warning."
    );
    
    // Range should point to the @type_only directive
    let expected_range = crate::support::range_for_token(&doc, text, "@type_only");
    assert_eq!(w.range, expected_range);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_input_field_deprecation() {
    let schema_content = r#"
        input CreateUserInput {
          username: String!
          oldField: String @deprecated(reason: "Use newField")
          newField: String
        }
        type Query {
          test(input: CreateUserInput): String
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query Test {
            test(input: { oldField: "value" })
        }
    "#;
    let doc = create_doc("file:///input_deprecated.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    let expected_message = "Input field 'oldField' is deprecated: Use newField".to_string();
    let warning = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected input deprecation warning '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
    // Range should point at oldField occurrence
    let expected = crate::support::range_for_token(&doc, text, "oldField");
    assert_eq!(warning.range.start, expected.start);
    assert_eq!(warning.range.end, expected.end);
}

#[test]
#[ntest::timeout(100)]
fn test_validation_unions_and_interfaces() {
    let schema_content = r#"
        interface Named {
          name: String!
        }
        type User implements Named {
          id: ID!
          name: String!
        }
        type Bot implements Named {
          id: ID!
          name: String!
          version: String!
        }
        union SearchResult = User | Bot
        type Query {
          search(term: String!): [SearchResult]
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    // Valid query with inline fragments
    let text = r#"
        query Search($term: String!) {
            search(term: $term) {
                ... on User { id name }
                ... on Bot { id name version }
            }
        }
    "#;
    let doc = create_doc("file:///valid_union.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Valid union query failed: {:?}",
        diagnostics
    );

    // Invalid: field not on union
    let text = r#"
        query {
            search(term: "foo") {
                id
            }
        }
    "#;
    let doc = create_doc("file:///invalid_union.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    let expected_message = "Field 'id' not found on type 'SearchResult'".to_string();
    let error = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected union missing field error '{}', got: {:?}",
                expected_message, diagnostics
            )
        });
    let expected = crate::support::range_for_token(&doc, text, "id");
    assert_eq!(error.range.start, expected.start);
    assert_eq!(error.range.end, expected.end);

    // Valid: field on interface
    let text = r#"
        query {
            search(term: "foo") {
                ... on Named { name }
            }
        }
    "#;
    let doc = create_doc("file:///valid_interface.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Valid interface query failed: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_alias_field_lookup() {
    let text = r#"
        query GetUsers {
            users {
                u: username
                email
            }
        }
    "#;
    let doc = create_doc("file:///alias.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics for aliased field lookup, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_block_strings_and_comments() {
    let text = r#"
        query GetUser($id: ID! = """123""") # This is a comment
        {
            node(id: $id) {
                id
            }
        }
    "#;
    let doc = create_doc("file:///quirks.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Block strings or comments caused issues: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_circular_fragments() {
    let text = r#"
        fragment FragA on User { ...FragB }
        fragment FragB on User { ...FragA }
    "#;
    let doc = create_doc("file:///circular.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    // Expect two circular fragment diagnostics, one for each spread participating in the cycle
    assert_eq!(diagnostics.len(), 2);

    // Diagnostic on FragB in FragA (line 1)
    assert!(diagnostics.iter().any(|d| d.range == range(1, 36, 1, 41)));

    // Diagnostic on FragA in FragB (line 2)
    assert!(diagnostics.iter().any(|d| d.range == range(2, 36, 2, 41)));
}

#[test]
#[ntest::timeout(100)]
fn test_validation_circular_fragments_three_way() {
    let text = r#"
        fragment A on User { ...B }
        fragment B on User { ...C }
        fragment C on User { ...A }
    "#;
    let doc = create_doc("file:///circular3.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    // Expect three circular fragment diagnostics, one for each spread in the cycle
    assert_eq!(diagnostics.len(), 3);

    // Diagnostic: A -> B (on line 1)
    assert!(diagnostics.iter().any(|d| d.range == range(1, 32, 1, 33)));

    // Diagnostic: B -> C (on line 2)
    assert!(diagnostics.iter().any(|d| d.range == range(2, 32, 2, 33)));

    // Diagnostic: C -> A (on line 3)
    assert!(diagnostics.iter().any(|d| d.range == range(3, 32, 3, 33)));
}
