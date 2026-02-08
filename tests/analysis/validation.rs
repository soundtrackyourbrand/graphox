use apollo_compiler::Schema;
use graphql_rust::features::completion::FragmentCompletionInfo;
use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString, Url};

use crate::support::{create_doc, get_valid_schema, range_for_token_at_index};

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

    assert_eq!(diagnostics.len(), 1);
    let error = &diagnostics[0];
    assert_eq!(
        error.message,
        "Field 'nonExistentField' not found on type 'User'"
    );
    assert_eq!(error.severity, Some(DiagnosticSeverity::ERROR));
    crate::support::assert_diag_range_equals(error, &crate::support::range_for_token(&doc, text, "nonExistentField"));
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

    assert_eq!(diagnostics.len(), 1);
    let warning = &diagnostics[0];
    assert_eq!(warning.message, "Field 'oldField' is deprecated: Use username instead");
    assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
    crate::support::assert_diag_range_equals(warning, &crate::support::range_for_token(&doc, text, "oldField"));
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

    assert_eq!(diagnostics.len(), 1);
    let error = &diagnostics[0];
    assert_eq!(
        error.message,
        "Field 'missingInAuthor' not found on type 'User'"
    );
    crate::support::assert_diag_range_equals(error, &crate::support::range_for_token(&doc, text, "missingInAuthor"));
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

    assert_eq!(diagnostics.len(), 1);
    let error = &diagnostics[0];
    assert_eq!(
        error.message,
        "Field 'missingInFragment' not found on type 'User'"
    );
    crate::support::assert_diag_range_equals(error, &crate::support::range_for_token(&doc, text, "missingInFragment"));
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

    assert_eq!(diagnostics.len(), 1);
    let error = &diagnostics[0];
    assert_eq!(
        error.message,
        "Field 'nonExistentOnUser' not found on type 'User'"
    );
    crate::support::assert_diag_range_equals(error, &crate::support::range_for_token(&doc, text, "nonExistentOnUser"));
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

    assert_eq!(diagnostics.len(), 1);
    let error = &diagnostics[0];
    assert_eq!(error.message, "Unknown fragment: UnknownFrag");
    crate::support::assert_diag_range_equals(error, &crate::support::range_for_token(&doc, text, "UnknownFrag"));
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

    assert_eq!(diagnostics.len(), 1);
    let warning = &diagnostics[0];
    assert_eq!(warning.code, Some(NumberOrString::String("type_only_used".to_string())));
    assert_eq!(
        warning.message,
        "Fragment 'UserFrag' is used but marked with @type_only. Remove @type_only to resolve this warning."
    );
    crate::support::assert_diag_range_equals(warning, &crate::support::range_for_token(&doc, text, "@type_only"));
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
            test(input: { username: "test", oldField: "value" })
        }
    "#;
    let doc = create_doc("file:///input_deprecated.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert_eq!(diagnostics.len(), 1);
    let warning = &diagnostics[0];
    assert_eq!(warning.message, "Input field 'oldField' is deprecated: Use newField");
    assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
    crate::support::assert_diag_range_equals(warning, &crate::support::range_for_token(&doc, text, "oldField"));
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
    assert_eq!(diagnostics.len(), 1);
    let error = &diagnostics[0];
    assert_eq!(error.message, "Field 'id' not found on type 'SearchResult'");
    crate::support::assert_diag_range_equals(error, &crate::support::range_for_token(&doc, text, "id"));

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
    assert!(diagnostics
        .iter()
        .any(|d| d.range == range_for_token_at_index(&doc, text, "FragB", 0)));

    // Diagnostic on FragA in FragB (line 2)
    assert!(diagnostics
        .iter()
        .any(|d| d.range == range_for_token_at_index(&doc, text, "FragA", 1)));
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
    assert!(diagnostics
        .iter()
        .any(|d| d.range == range_for_token_at_index(&doc, text, "B", 0)));

    // Diagnostic: B -> C (on line 2)
    assert!(diagnostics
        .iter()
        .any(|d| d.range == range_for_token_at_index(&doc, text, "C", 0)));

    // Diagnostic: C -> A (on line 3)
    assert!(diagnostics
        .iter()
        .any(|d| d.range == range_for_token_at_index(&doc, text, "A", 1)));
}
