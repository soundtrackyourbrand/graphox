use crate::support::create_doc;
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp_server::ls_types::DiagnosticSeverity;

#[test]
#[ntest::timeout(500)]
fn test_apollo_validation_missing_required_argument() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
            username: String!
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = "query GetUser { user { id } }";
    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    // Apollo compiler should report missing required argument 'id'
    assert_eq!(
        diagnostics.len(),
        1,
        "Expected exactly 1 diagnostic, got: {:?}",
        diagnostics
    );

    let d = &diagnostics[0];
    assert!(d.message.contains("Apollo Validation Error"));
    assert!(d.message.contains("id") && d.message.contains("not provided"));
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));

    crate::support::assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "user { id }"),
    );
}

#[test]
#[ntest::timeout(500)]
fn test_apollo_validation_type_mismatch() {
    let schema_content = r#"
        type Query {
            user(id: ID!): User
        }
        type User {
            id: ID!
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = "query GetUser { user(id: { some: \"object\" }) { id } }";
    let doc = create_doc("file:///test.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert_eq!(
        diagnostics.len(),
        1,
        "Expected exactly 1 diagnostic, got: {:?}",
        diagnostics
    );

    let d = &diagnostics[0];
    assert!(d.message.contains("Apollo Validation Error"));
    assert!(d.message.contains("expected value of type ID!"));

    crate::support::assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, r#"{ some: "object" }"#),
    );
}
