use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    lsp_request_hover, lsp_request_typed, make_temp_project_with_schema, pos, write_project_file,
};
use apollo_compiler::Schema;
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp::lsp_types::*;

#[test]
fn test_variable_used_in_fragment_spread() {
    let schema_content = r#"
        type User { id: ID! username: String }
        type Query { user(id: ID): User }
        directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

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
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    crate::support::assert_no_diagnostics(&diagnostics);
}

#[test]
fn test_variable_used_transitively_in_nested_fragments() {
    let schema_content = r#"
        type User { id: ID! username: String }
        type Query { user(id: ID): User }
        directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

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
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    crate::support::assert_no_diagnostics(&diagnostics);
}

#[test]
fn test_variable_unused_even_with_fragments() {
    let schema_content = r#"
        type User { id: ID! username: String }
        type Query { user(id: ID): User }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

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
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    // Total diagnostics: 1 (our unused_variable check)
    // Wait, apollo-compiler also reports unused variables.
    // Let's check how many we actually get.
    assert!(!diagnostics.is_empty());
    let d = diagnostics
        .iter()
        .find(|d| d.message == "Unused variable: $unused")
        .expect("Should find unused variable diagnostic");
    assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
    // Range points to the variable name (including $)
    crate::support::assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, query_text, "$unused"),
    );
}

#[test]
fn test_undefined_variable_direct() {
    let schema_content = r#"
        type User { id: ID! username: String }
        type Query { user(id: ID): User }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = r#"
        query GetUser($id: ID) {
            user(id: $undefined) {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    // apollo-compiler and our rule both report this.
    assert!(!diagnostics.is_empty());
    let d = diagnostics
        .iter()
        .find(|d| d.message == "Undefined variable: $undefined")
        .expect("Should find undefined variable diagnostic");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    crate::support::assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, query_text, "$undefined"),
    );
}

#[test]
fn test_undefined_variable_in_fragment_spread() {
    let schema_content = r#"
        type User { id: ID! username: String }
        type Query { user(id: ID): User }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = r#"
        query GetUser($id: ID) {
            user(id: $id) {
                ...UserFields
            }
        }
        
        fragment UserFields on User {
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert!(!diagnostics.is_empty());
    let d = diagnostics
        .iter()
        .find(|d| d.message == "Undefined variable: $admin (required by fragment 'UserFields')")
        .expect("Should find undefined variable diagnostic");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    // UserFields spread is on line 3, point to the fragment name
    crate::support::assert_diag_range_equals(
        d,
        &crate::support::range_for_token_at_index(&doc, query_text, "UserFields", 0),
    );
}

#[tokio::test]
async fn test_fragment_hover_requirements() {
    let schema = "type User { id: ID! name: String friend(id: ID): User } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let frag_text = "fragment UserFields on User { friend(id: $friendId) { name } }";
    write_project_file(&dir, "frag.graphql", frag_text);

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { ...UserFields } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Hover over ...UserFields
    let result = lsp_request_hover(&mut service, query_uri.clone(), pos(0, 18)).await;

    let hover = result.expect("Expected hover");
    let value = match hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup content"),
    };

    assert!(
        value.contains("**Requires Variables:**"),
        "Hover should contain requirements header"
    );
    assert!(
        value.contains("$friendId"),
        "Hover should contain $friendId"
    );
    assert!(value.contains("ID"), "Hover should contain ID");
}

#[tokio::test]
async fn test_fragment_completion_requirements() {
    let schema = "type User { id: ID! name: String friend(id: ID): User } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let frag_text = "fragment UserFields on User { friend(id: $friendId) { name } }";
    write_project_file(&dir, "frag.graphql", frag_text);

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { ... } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Completion after ...
    let result = lsp_request_completion(&mut service, query_uri.clone(), pos(0, 16)).await;

    let completion = crate::support::completion_items_array(&result);

    let item = completion
        .iter()
        .find(|i| i.label == "UserFields")
        .expect("Should find UserFields completion");
    let doc = match item.documentation.as_ref().unwrap() {
        Documentation::MarkupContent(m) => &m.value,
        _ => panic!("Expected markup content"),
    };

    assert!(
        doc.contains("**Requires Variables:**"),
        "Completion doc should contain requirements header"
    );
    assert!(
        doc.contains("$friendId"),
        "Completion doc should contain $friendId"
    );
    assert!(doc.contains("ID"), "Completion doc should contain ID");
}

#[test]
fn test_variable_in_directive_requirement() {
    let schema_content = r#"
        type User { id: ID! name: String }
        type Query { me: User }
        directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql").unwrap();

    let frag_text = "fragment UserFields on User { name @include(if: $admin) }";
    let doc = create_doc("file:///test.graphql", frag_text);

    let vars = doc.get_fragment_variable_types("UserFields", &schema);
    assert_eq!(vars.get("admin").unwrap(), "Boolean!");
}

#[tokio::test]
async fn test_variable_references_including_fragments() {
    let schema = "type User { id: ID! name: String } type Query { me: User } directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let frag_text = "fragment UserFields on User { name @include(if: $admin) }";
    let frag_uri = write_project_file(&dir, "frag.graphql", frag_text);

    let query_text = r#"
        query GetMe($admin: Boolean!) {
            me {
                id
                ...UserFields
            }
        }
    "#;
    let query_uri = write_project_file(&dir, "query.graphql", query_text);

    let (mut service, _) = create_initialized_lsp_service(config).await;

    // Open files
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Request references for $admin in GetMe
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(1, 21),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");

    // We expect 2 locations:
    // 1. Declaration in query.graphql
    // 2. Usage in frag.graphql

    assert!(
        locations.iter().any(|l| l.uri == query_uri),
        "Expected reference in query.graphql"
    );
    assert!(
        locations.iter().any(|l| l.uri == frag_uri),
        "Expected reference in frag.graphql"
    );
}

#[test]
fn test_fragment_variables_not_undefined_in_isolation() {
    let schema = crate::support::get_valid_schema();

    let frag_text = r#"
        fragment UserFields on User {
            id
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", frag_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);
    crate::support::assert_no_diagnostics(&diagnostics);
}

#[test]
fn test_variable_used_only_in_directive() {
    let schema_content = r#"
        type User {
            id: ID!
            name: String
        }
        type Query {
            me: User
        }
        directive @skip(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .expect("Failed to parse schema")
        .validate()
        .expect("Schema validation failed");

    let query_text = r#"
        query GetMe($skipName: Boolean!) {
            me {
                id
                name @skip(if: $skipName)
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    assert_eq!(diagnostics.len(), 0);
}
