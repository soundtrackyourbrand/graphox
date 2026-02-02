use tower_lsp::lsp_types::*;
use graphql_rust::DocumentState;
use apollo_compiler::Schema;
use std::sync::OnceLock;

// Shared schema for tests
static SCHEMA: OnceLock<Schema> = OnceLock::new();

fn get_schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql")
            .expect("Failed to read schema file");
        Schema::parse(&schema_content, "schema.graphql").expect("Failed to parse schema")
    })
}

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_graphql::LANGUAGE.into()).unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
fn test_validation_valid_query() {
    let schema = get_schema();
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
    let diagnostics = doc.get_semantic_diagnostics(schema);
    
    assert!(diagnostics.is_empty(), "Expected no diagnostics, got: {:?}", diagnostics);
}

#[test]
fn test_validation_missing_field() {
    let schema = get_schema();
    let text = r#"
        query GetUser {
            users {
                id
                nonExistentField
            }
        }
    "#;
    let doc = create_doc("file:///missing.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(schema);
    
    let error = diagnostics.iter().find(|d| d.message.contains("not found"));
    assert!(error.is_some(), "Expected 'not found' error");
    assert_eq!(error.unwrap().severity, Some(DiagnosticSeverity::ERROR));
    assert!(error.unwrap().message.contains("nonExistentField"));
    assert!(error.unwrap().message.contains("User")); // Should mention parent type
}

#[test]
fn test_validation_deprecated_field() {
    let schema = get_schema();
    let text = r#"
        query GetUser {
            users {
                id
                oldField
            }
        }
    "#;
    let doc = create_doc("file:///deprecated.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(schema);
    
    let warning = diagnostics.iter().find(|d| d.message.contains("deprecated"));
    assert!(warning.is_some(), "Expected 'deprecated' warning");
    assert_eq!(warning.unwrap().severity, Some(DiagnosticSeverity::WARNING));
    assert!(warning.unwrap().message.contains("oldField"));
    assert!(warning.unwrap().message.contains("Use username instead"), "Message should contain reason: {}", warning.unwrap().message);
}

#[test]
fn test_validation_nested_missing_field() {
    let schema = get_schema();
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
    let diagnostics = doc.get_semantic_diagnostics(schema);
    
    let error = diagnostics.iter().find(|d| d.message.contains("missingInAuthor"));
    assert!(error.is_some(), "Expected nested missing field error");
    assert!(error.unwrap().message.contains("User")); // Author is User
}

#[test]
fn test_validation_fragment() {
    let schema = get_schema();
    let text = r#"
        fragment UserFrag on User {
            id
            missingInFragment
        }
    "#;
    let doc = create_doc("file:///fragment.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(schema);
    
    let error = diagnostics.iter().find(|d| d.message.contains("missingInFragment"));
    assert!(error.is_some(), "Expected error in fragment");
    assert!(error.unwrap().message.contains("User"));
}
