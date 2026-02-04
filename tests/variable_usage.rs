use apollo_compiler::Schema;
use graphql_rust::DocumentState;
use std::sync::OnceLock;
use tower_lsp::lsp_types::*;

static SCHEMA: OnceLock<Schema> = OnceLock::new();

fn get_schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let schema_content = r#"
            type User {
                id: ID!
                username: String!
                email: String!
            }
            type Query {
                user(id: ID): User
            }
        "#;
        Schema::parse(schema_content, "schema.graphql").expect("Failed to parse schema")
    })
}

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
fn test_variable_used_in_fragment_spread() {
    let schema = get_schema();

    let query_text = r#"
        query GetUser($id: ID, $admin: Boolean) {
            user(id: $id) {
                ...UserFields
            }
        }
        
        fragment UserFields on User {
            id
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable"))
        .collect();

    assert!(
        unused_vars.is_empty(),
        "Expected no unused variables, but found: {:?}",
        unused_vars
    );
}

#[test]
fn test_variable_used_transitively_in_nested_fragments() {
    let schema = get_schema();

    let query_text = r#"
        query GetUser($id: ID, $admin: Boolean) {
            user(id: $id) {
                ...Level1
            }
        }
        
        fragment Level1 on User {
            ...Level2
        }
        
        fragment Level2 on User {
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable"))
        .collect();

    assert!(
        unused_vars.is_empty(),
        "Expected no unused variables in transitive case, but found: {:?}",
        unused_vars
    );
}

#[test]
fn test_variable_unused_even_with_fragments() {
    let schema = get_schema();

    let query_text = r#"
        query GetUser($id: ID, $unused: String) {
            user(id: $id) {
                ...UserFields
            }
        }
        
        fragment UserFields on User {
            id
            username
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable: $unused"))
        .collect();

    assert_eq!(
        unused_vars.len(),
        1,
        "Expected one unused variable ($unused)"
    );
}
