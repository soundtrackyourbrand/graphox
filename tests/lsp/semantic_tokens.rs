#![allow(unused_imports)]
use tower_lsp::lsp_types::{Position, Range, SemanticToken, SemanticTokenType};

use crate::support::create_doc;
use crate::support::fixtures::{user_schema, user_with_deprecated_field_schema};
use graphox::features::semantic_tokens::DocumentSemanticTokens;

// Token type indices matching SemanticTokenKind enum
#[allow(dead_code)]
const TOKEN_VARIABLE: u32 = 0;
#[allow(dead_code)]
const TOKEN_TYPE: u32 = 1;
#[allow(dead_code)]
const TOKEN_STRING: u32 = 2;
const TOKEN_KEYWORD: u32 = 3;
const TOKEN_PROPERTY: u32 = 4;
const TOKEN_FUNCTION: u32 = 5;
const TOKEN_ENUM: u32 = 6;

fn count_tokens_by_type(tokens: &[SemanticToken], token_type: u32) -> usize {
    tokens.iter().filter(|t| t.token_type == token_type).count()
}

#[test]
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_operation_name() {
    // Test that operation names are tokenized as FUNCTION
    let text = r#"
        subscription SonarZone($zoneId: ID!) {
            soundZoneUpdate(input: { soundZone: $zoneId }) {
                soundZone {
                    id
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have at least one FUNCTION token for "SonarZone"
    let fn_count = count_tokens_by_type(&tokens, TOKEN_FUNCTION);
    assert!(
        fn_count >= 1,
        "Expected at least 1 function token, got {}",
        fn_count
    );

    // Should have KEYWORD tokens for "subscription"
    let kw_count = count_tokens_by_type(&tokens, TOKEN_KEYWORD);
    assert!(
        kw_count >= 1,
        "Expected at least 1 keyword token, got {}",
        kw_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_enum_values() {
    let text = r#"
        enum Role {
            ADMIN
            USER
            GUEST
        }

        query GetUser {
            user(role: ADMIN) {
                role
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have ENUM tokens for ADMIN, USER, GUEST
    let enum_count = count_tokens_by_type(&tokens, TOKEN_ENUM);
    assert!(
        enum_count >= 3,
        "Expected at least 3 enum tokens, got {}",
        enum_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_field_properties() {
    let text = r#"
        query GetUser {
            user {
                id
                name
                email
                posts {
                    title
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have PROPERTY tokens for field names: user, id, name, email, posts, title
    let prop_count = count_tokens_by_type(&tokens, TOKEN_PROPERTY);
    assert!(
        prop_count >= 6,
        "Expected at least 6 property tokens, got {}",
        prop_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_directives() {
    let text = r#"
        query GetUser($skip: Boolean!) {
            user @skip(if: $skip) {
                id
                name
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have PROPERTY tokens for directive names: @skip, @include would be property
    // And also for argument names: if
    let prop_count = count_tokens_by_type(&tokens, TOKEN_PROPERTY);
    assert!(
        prop_count >= 1,
        "Expected at least 1 property token for directives, got {}",
        prop_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_arguments() {
    let text = r#"
        query GetUser($id: ID!, $includeEmail: Boolean!) {
            user(id: $id, includeEmail: $includeEmail) {
                id
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have PROPERTY tokens for argument names: id, includeEmail
    let prop_count = count_tokens_by_type(&tokens, TOKEN_PROPERTY);
    assert!(
        prop_count >= 2,
        "Expected at least 2 property tokens for arguments, got {}",
        prop_count
    );

    // Should have VARIABLE tokens for $id and $includeEmail
    let var_count = count_tokens_by_type(&tokens, TOKEN_VARIABLE);
    assert!(
        var_count >= 2,
        "Expected at least 2 variable tokens, got {}",
        var_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_fragment_definition() {
    let text = r#"
        fragment UserFields on User {
            id
            name
            email
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have FUNCTION token for fragment name "UserFields"
    let fn_count = count_tokens_by_type(&tokens, TOKEN_FUNCTION);
    assert!(
        fn_count >= 1,
        "Expected at least 1 function token for fragment name, got {}",
        fn_count
    );

    // Should have TYPE token for "User"
    let type_count = count_tokens_by_type(&tokens, TOKEN_TYPE);
    assert!(
        type_count >= 1,
        "Expected at least 1 type token, got {}",
        type_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_all_operation_types() {
    let text = r#"
        query GetUsers {
            users { id }
        }

        mutation CreateUser {
            createUser { id }
        }

        subscription OnMessage {
            newMessage { id }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have 3 KEYWORD tokens for query, mutation, subscription
    let kw_count = count_tokens_by_type(&tokens, TOKEN_KEYWORD);
    assert!(
        kw_count >= 3,
        "Expected at least 3 keyword tokens, got {}",
        kw_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_embedded_tsx() {
    // Test the exact example from the user's question
    let text = r#"
        export const ZoneUpdateDoc = graphql(/* GraphQL */ `
            subscription SonarZone($zoneId: ID!) {
                soundZoneUpdate(input: { soundZone: $zoneId }) {
                    soundZone {
                        ...SonarZone
                    }
                }
            }
        `);
    "#;
    let doc = create_doc("file:///test.tsx", text);
    let tokens = doc.get_semantic_tokens();

    // Verify we get tokens from the embedded GraphQL
    assert!(!tokens.is_empty(), "Expected tokens from embedded GraphQL");

    // Should have FUNCTION token for "SonarZone"
    let fn_count = count_tokens_by_type(&tokens, TOKEN_FUNCTION);
    assert!(
        fn_count >= 1,
        "Expected function token for operation name, got {}",
        fn_count
    );

    // Should have VARIABLE token for $zoneId
    let var_count = count_tokens_by_type(&tokens, TOKEN_VARIABLE);
    assert!(var_count >= 1, "Expected variable token, got {}", var_count);

    // Should have TYPE token for ID
    let type_count = count_tokens_by_type(&tokens, TOKEN_TYPE);
    assert!(
        type_count >= 1,
        "Expected type token for ID, got {}",
        type_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_mixed_fragment_spread() {
    let text = r#"
        fragment NameParts on User {
            firstName
            lastName
        }

        query GetFullName {
            user {
                ...NameParts
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have FUNCTION token for fragment definition "NameParts"
    let fn_count = count_tokens_by_type(&tokens, TOKEN_FUNCTION);
    assert!(
        fn_count >= 1,
        "Expected at least 1 function token, got {}",
        fn_count
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_semantic_tokens_inline_fragment() {
    let text = r#"
        query GetUser {
            user {
                ... on User {
                    id
                    name
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let tokens = doc.get_semantic_tokens();

    // Should have tokens for fields
    assert!(!tokens.is_empty());
}
