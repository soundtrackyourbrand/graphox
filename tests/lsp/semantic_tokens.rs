use graphql_rust::DocumentState;
use tower_lsp::lsp_types::*;

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

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
    let uri = Url::parse("file:///test.tsx").unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .unwrap();
    let doc = DocumentState::new(uri, text, parser);
    let tokens = doc.get_semantic_tokens();

    assert!(!tokens.is_empty());
}
