#![allow(unused_imports)]
use graphql_rust::features::folding_range::DocumentFoldingRange;
use tower_lsp::lsp_types::FoldingRangeKind;

use crate::support::create_doc;

#[test]
fn test_folding_ranges_operation() {
    let text = r#"
query GetUser($id: ID!) {
    user(id: $id) {
        id
        name
        email
    }
}
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    // Should have folding ranges for:
    // - operation_definition
    // - selection_set (user)
    // - variable_definitions
    assert!(!ranges.is_empty());

    // Check we have at least one region kind
    let has_region = ranges
        .iter()
        .any(|r| r.kind == Some(FoldingRangeKind::Region));
    assert!(has_region, "Should have at least one region folding range");
}

#[test]
fn test_folding_ranges_fragment() {
    let text = r#"
fragment UserFields on User {
    id
    name
    email
    profile {
        bio
        avatar
    }
}
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    assert!(!ranges.is_empty());

    // Fragment should be foldable
    let has_fragment_fold = ranges
        .iter()
        .any(|r| r.kind == Some(FoldingRangeKind::Region) && r.end_line > r.start_line);
    assert!(has_fragment_fold, "Fragment definition should be foldable");
}

#[test]
fn test_folding_ranges_schema() {
    let text = r#"
type User {
    id: ID!
    name: String!
    email: String!
    posts: [Post!]!
}

enum Role {
    ADMIN
    USER
    GUEST
}

input CreateUserInput {
    name: String!
    email: String!
    role: Role!
}
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    // Should have folding ranges for type, enum, and input definitions
    assert!(
        ranges.len() >= 3,
        "Should have at least 3 folding ranges for type, enum, and input"
    );
}

#[test]
fn test_folding_ranges_nested_selection_sets() {
    let text = r#"
query GetUserWithPosts {
    user {
        id
        name
        posts {
            id
            title
            comments {
                id
                text
            }
        }
    }
}
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    // Should have multiple nested selection sets
    assert!(
        ranges.len() >= 3,
        "Should have multiple nested selection sets"
    );
}

#[test]
fn test_folding_ranges_arguments() {
    let text = r#"
query GetUser {
    user(
        id: "123"
        includeDeleted: false
        sortBy: CREATED_AT
    ) {
        id
    }
}
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    // Arguments list should be foldable
    let has_args_fold = ranges
        .iter()
        .any(|r| r.kind == Some(FoldingRangeKind::Region));
    assert!(has_args_fold, "Arguments should be foldable");
}

#[test]
fn test_folding_ranges_tsx() {
    let text = r#"
const query = gql`
    query GetUser($id: ID!) {
        user(id: $id) {
            id
            name
            email
        }
    }
`;
    "#;
    let doc = create_doc("file:///test.tsx", text);
    let ranges = doc.get_folding_ranges();

    // Should extract GraphQL from TSX and provide folding ranges
    assert!(
        !ranges.is_empty(),
        "Should have folding ranges from GraphQL in TSX"
    );
}

#[test]
fn test_folding_ranges_single_line_no_fold() {
    let text = r#"
query GetUser { user { id } }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    // Single-line constructs should not be foldable
    assert!(
        ranges.is_empty(),
        "Single-line query should not have folding ranges"
    );
}

#[test]
fn test_folding_ranges_inline_fragment() {
    let text = r#"
query GetNode {
    node {
        ... on User {
            id
            name
        }
        ... on Post {
            id
            title
        }
    }
}
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let ranges = doc.get_folding_ranges();

    // Inline fragments should be foldable
    assert!(
        ranges.len() >= 3,
        "Should have folding ranges for inline fragments"
    );
}
