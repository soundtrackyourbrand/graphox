#![allow(unused_imports)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::expect_fun_call)]
#![allow(clippy::duplicate_mod)]
#![allow(clippy::collapsible_if)]

use apollo_compiler::Schema;
use graphox::DocumentState;
use graphox::features::definition::DocumentDefinition;
use graphox::features::diagnostics::DocumentDiagnostics;
// DocumentState not referenced directly; tests use `create_doc` helper
use crate::support::create_doc;
use tower_lsp::lsp_types::*;

fn create_ts_doc(text: &str) -> DocumentState {
    create_doc("file:///test.ts", text)
}

#[test]
#[ntest::timeout(300)]
fn test_multibyte_range_calculation() {
    let schema_content = r#"
        type Query {
          oldField: String @deprecated(reason: "Use newField")
          newField: String
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    // 🚀 is 4 bytes in UTF-8, 1 char in Unicode, 2 code units in UTF-16
    let text = r#"
// 🚀 emoji shift
const q = gql`
  query {
    oldField
  }
`;
"#;
    let doc = create_ts_doc(text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert!(d.message.contains("oldField"));

    // Line 4 (0-indexed), "    oldField"
    // "    " is 4 spaces
    // The line is: "    oldField"
    // So character should be 4.
    assert_eq!(d.range.start.line, 4);
    assert_eq!(d.range.start.character, 4);
    assert_eq!(d.range.end.character, 12);
}

#[test]
#[ntest::timeout(300)]
fn test_emoji_in_query_comment() {
    let schema_content = r#"
        type Query {
          field: String
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
const q = gql`
  query {
    # 🚀 comment with emoji
    field
    unknownField
  }
`;
"#;
    let doc = create_ts_doc(text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    let error = diagnostics
        .iter()
        .find(|d| d.message.contains("unknownField") && d.range.start.line == 5)
        .expect("Should find precise error for unknownField");

    // Line 5: "    unknownField"
    // Character should be 4.
    assert_eq!(error.range.start.line, 5);
    assert_eq!(error.range.start.character, 4);
}

#[test]
#[ntest::timeout(300)]
fn test_goto_definition_with_emoji() {
    let text = r#"
fragment UserFrag on User { id }
const q = gql`
  query {
    # 🚀 emoji shift
    ...UserFrag
  }
`;
"#;
    let doc = create_ts_doc(text);

    // Line 5: "    ...UserFrag"
    // "...UserFrag" starts at column 4
    // "UserFrag" starts at column 7
    let pos = Position::new(5, 7);
    let symbol = doc.get_symbol_at_position(pos);
    assert_eq!(symbol, Some("UserFrag".to_string()));
}

#[test]
#[ntest::timeout(300)]
fn test_cjk_characters_in_strings() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
                name
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Should handle queries with ASCII identifiers when CJK is in comments/strings"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_arabic_characters_in_strings() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
                name
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Should handle queries normally when Arabic is in comments/strings"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_emoji_in_variable_names() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query GetUser($🔑id: ID!) {
            user(id: $🔑id) {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        !diagnostics.is_empty(),
        "Emoji in variable names should be valid in GraphQL"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_combining_characters() {
    let schema_content = r#"
        type Query {
            user: User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            user {
                id
                # Combining character e followed by acute accent
                name
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    crate::support::assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_zero_width_characters() {
    let schema_content = r#"
        type Query {
            user: User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            user {
                id
                name
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    crate::support::assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_cyrillic_characters_in_strings() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
                name
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Should handle queries normally when Cyrillic is in comments/strings"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_greek_characters_in_strings() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
            name: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
                name
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Should handle queries normally when Greek is in comments/strings"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_mixed_multibyte_in_strings() {
    let schema_content = r#"
        type Query {
            user: User
        }
        type User {
            id: ID!
            greeting: String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            user {
                id
                greeting
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    crate::support::assert_no_diagnostics(&diagnostics);
}
