#![allow(unused_imports)]
use tower_lsp::lsp_types::{Position, Range};

use crate::support::create_doc;

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
