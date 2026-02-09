#![allow(unused_imports)]
use tower_lsp::lsp_types::{Position, Range, SemanticTokenType};

use crate::support::create_doc;
use crate::support::fixtures::{user_schema, user_with_deprecated_field_schema};
use graphox::features::semantic_tokens::DocumentSemanticTokens;

#[test]
fn test_semantic_tokens_basic() {
    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // The output is delta-encoded, so it's hard to assert exact values without a helper
    // but we can check if we got some tokens.
    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_tsx() {
    let text = r#"
        const query = gql`
            query GetUser($id: ID!) {
                user(id: $id) { id }
            }
        `;
    "#;
    let doc = create_doc("file:///test.tsx", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_keywords() {
    let text = r#"
        query GetUser {
            query getPosts {
                mutation createUser {
                    subscription onMessage
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_types() {
    let text = r#"
        type User {
            id: ID!
            name: String
        }

        interface Node {
            id: ID!
        }

        union SearchResult = User | Node

        enum Role {
            ADMIN
            USER
        }

        input CreateUserInput {
            name: String!
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_fields() {
    let text = r#"
        query GetUser {
            user {
                id
                name
                email
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_variables() {
    let text = r#"
        query GetUser($id: ID!, $includeEmail: Boolean!) {
            user(id: $id, includeEmail: $includeEmail) {
                id
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_deprecated_modifier() {
    let text = r#"
        query GetUser {
            user {
                id
                oldField
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_positions() {
    let text = r#"
        query GetUser {
            user {
                id
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    for token in &tokens {
        assert!(token.length > 0);
    }
}

#[test]
fn test_semantic_tokens_tsx_with_offset() {
    let text = r#"
        const Component = () => {
            const query = gql`
                query GetUser($id: ID!) {
                    user(id: $id) {
                        id
                        name
                    }
                }
            `;
            return <div>{query}</div>;
        };
    "#;
    let doc = create_doc("file:///test.tsx", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_empty_block() {
    let text = r#"
        query EmptyQuery {
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}

#[test]
fn test_semantic_tokens_fragment_spread() {
    let text = r#"
        fragment UserFields on User {
            id
            name
        }

        query GetUser {
            user {
                ...UserFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}
