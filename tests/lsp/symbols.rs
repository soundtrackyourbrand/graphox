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
fn test_document_symbols() {
    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
                username
            }
        }

        fragment UserFields on User {
            id
            email
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let symbols = doc.get_symbols();

    assert!(symbols
        .iter()
        .any(|s| s.name == "GetUser" && s.kind == SymbolKind::STRUCT));
    assert!(symbols
        .iter()
        .any(|s| s.name == "UserFields" && s.kind == SymbolKind::STRUCT));

    // Check that we have fields too (if implemented as children or flat)
    // Current implementation is flat STRUCTs for operations and fragments
    assert_eq!(symbols.len(), 2);
}

#[test]
fn test_document_symbols_tsx() {
    let text = r#"
        const query = gql`
            query GetUser {
                me { id }
            }
        `;

        function Component() {
            const frag = graphql`
                fragment MyFrag on User {
                    username
                }
            `;
        }
    "#;
    let uri = Url::parse("file:///test.tsx").unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .unwrap();
    let doc = DocumentState::new(uri, text, parser);
    let symbols = doc.get_symbols();

    assert!(symbols.iter().any(|s| s.name == "GetUser"));
    assert!(symbols.iter().any(|s| s.name == "MyFrag"));
}
