use tower_lsp::lsp_types::*;

use crate::support::create_doc;
use graphox::features::symbols::DocumentSymbols;

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

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "GetUser" && s.kind == SymbolKind::STRUCT)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UserFields" && s.kind == SymbolKind::STRUCT)
    );

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
    let doc = create_doc("file:///test.tsx", text);
    let symbols = doc.get_symbols();

    assert!(symbols.iter().any(|s| s.name == "GetUser"));
    assert!(symbols.iter().any(|s| s.name == "MyFrag"));
}

#[test]
fn test_document_symbols_hierarchy() {
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
    let symbols = doc.get_symbols();

    let query_symbol = symbols.iter().find(|s| s.name == "GetUserWithPosts");
    assert!(query_symbol.is_some(), "Should find query symbol");

    if let Some(query) = query_symbol {
        assert!(
            query.range.start.line < query.range.end.line,
            "Query symbol should have a valid range"
        );
    }
}

#[test]
fn test_document_symbols_kinds() {
    let text = r#"
        query GetUser {
            user { id }
        }

        mutation CreateUser {
            createUser { id }
        }

        subscription OnMessage {
            messageAdded { id }
        }

        fragment UserFields on User {
            id
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let symbols = doc.get_symbols();

    let query_symbol = symbols.iter().find(|s| s.name == "GetUser");
    let mutation_symbol = symbols.iter().find(|s| s.name == "CreateUser");
    let subscription_symbol = symbols.iter().find(|s| s.name == "OnMessage");
    let fragment_symbol = symbols.iter().find(|s| s.name == "UserFields");

    assert!(query_symbol.is_some(), "Should find query symbol");
    assert!(mutation_symbol.is_some(), "Should find mutation symbol");
    assert!(
        subscription_symbol.is_some(),
        "Should find subscription symbol"
    );
    assert!(fragment_symbol.is_some(), "Should find fragment symbol");
}

#[test]
fn test_document_symbols_range() {
    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
                name
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let symbols = doc.get_symbols();

    for symbol in &symbols {
        assert!(
            symbol.range.start.line <= symbol.range.end.line,
            "Symbol {} should have valid range",
            symbol.name
        );
        assert!(
            symbol.range.start.character <= symbol.range.end.character,
            "Symbol {} should have valid character range",
            symbol.name
        );
    }
}

#[test]
fn test_document_symbols_fragment_spreads() {
    let text = r#"
        fragment UserFields on User {
            id
            name
            email
        }

        fragment ProfileFragment on User {
            profile {
                bio
                avatar
            }
        }

        query GetUser {
            user {
                ...UserFields
                ...ProfileFragment
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);
    let symbols = doc.get_symbols();

    let user_fields = symbols.iter().find(|s| s.name == "UserFields");
    let profile_fragment = symbols.iter().find(|s| s.name == "ProfileFragment");

    assert!(user_fields.is_some(), "Should find UserFields fragment");
    assert!(profile_fragment.is_some(), "Should find ProfileFragment");
}
