use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_typed,
    make_temp_project_with_schema, range_for_token, with_cursor, write_project_file,
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_field_references() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the schema file containing the field definition
    let (schema_text, position) = with_cursor("type User { id: ID! na|me: String }");
    let schema_uri = write_project_file(&dir, "schema.graphql", &schema_text);
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // 2. Create and Open a query file that uses the field
    let query_text = "query GetUser { user { name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Find References on "name" in the schema file
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext { include_declaration: true },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    // should include definition + usage
    assert!(locations.len() >= 2, "Expected at least 2 locations, got {}", locations.len());

    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);
    let expected_range = range_for_token(&schema_doc, &schema_text, "name");

    let has_def = locations
        .iter()
        .any(|l| l.uri == schema_uri && l.range == expected_range);
    let has_usage = locations
        .iter()
        .any(|l| l.uri == query_uri);

    assert!(has_def, "Missing field definition in references");
    assert!(has_usage, "Missing field usage in references");
}

#[tokio::test]
async fn test_directive_references() {
    let (dir, config) = make_temp_project_with_schema(
        "directive @foo on FIELD_DEFINITION type Query { user: User } type User { id: ID }",
        "**/*.graphql",
    );
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the directive definition file
    let (def_text, position) = with_cursor("directive @fo|o on FIELD_DEFINITION");
    let def_uri = write_project_file(&dir, "directive.graphql", &def_text);
    lsp_did_open(&mut service, def_uri.clone(), "graphql", 1, &def_text).await;

    // 2. Create and Open a query file that uses the directive
    let query_text = "query { user @foo { id } }";
    let query_uri = write_project_file(&dir, "query_with_directive.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Find References on "foo" in the directive definition
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: def_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext { include_declaration: true },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    assert!(locations.len() >= 2, "Expected at least 2 locations, got {}", locations.len());

    let def_doc = create_doc(def_uri.as_str(), &def_text);
    let expected_range = range_for_token(&def_doc, &def_text, "foo");

    let has_def = locations.iter().any(|l| l.uri == def_uri && l.range == expected_range);
    let has_usage = locations.iter().any(|l| l.uri == query_uri && l.range.start.line == 0);

    assert!(has_def, "Missing directive definition in references");
    assert!(has_usage, "Missing directive usage in references");
}
