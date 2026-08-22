use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_typed,
    make_temp_project_with_schema, range_for_token, with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
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
            text_document: TextDocumentIdentifier {
                uri: schema_uri.clone(),
            },
            position,
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
    // should include definition + usage
    assert!(
        locations.len() >= 2,
        "Expected at least 2 locations, got {}",
        locations.len()
    );

    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);
    let expected_range = range_for_token(&schema_doc, &schema_text, "name");

    let has_def = locations
        .iter()
        .any(|l| l.uri == schema_uri && l.range == expected_range);
    let has_usage = locations.iter().any(|l| l.uri == query_uri);

    assert!(has_def, "Missing field definition in references");
    assert!(has_usage, "Missing field usage in references");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_directive_references() {
    // Schema with directive and type
    let schema_text =
        "directive @foo on FIELD_DEFINITION type Query { user: User } type User { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the schema file with directive
    let (schema_with_cursor, position) = with_cursor(
        "directive @fo|o on FIELD_DEFINITION type Query { user: User } type User { id: ID }",
    );
    let schema_uri = write_project_file(&dir, "schema.graphql", &schema_with_cursor);
    lsp_did_open(
        &mut service,
        schema_uri.clone(),
        "graphql",
        1,
        &schema_with_cursor,
    )
    .await;

    // 2. Create and Open a query file that uses the directive
    let query_text = "query { user @foo { id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Find References on "foo" in the directive definition
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_uri.clone(),
            },
            position,
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
    assert!(
        locations.len() >= 2,
        "Expected at least 2 locations, got {}",
        locations.len()
    );

    let has_def = locations.iter().any(|l| l.uri == schema_uri);
    let has_usage = locations.iter().any(|l| l.uri == query_uri);

    assert!(has_def, "Missing directive definition in references");
    assert!(has_usage, "Missing directive usage in references");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_references_type_aware() {
    // Schema with two types that have the same field name
    // We test that finding references for User.name only finds User.name usages,
    // NOT Product.name usages
    let schema_text = "type Query { user: User, product: Product } type User { id: ID! name: String } type Product { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open the schema file
    let schema_uri = write_project_file(&dir, "schema.graphql", schema_text);
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    // 2. Create query file that uses name on both User and Product
    let query_text = "query { user { name } product { name } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Find references on User.name using with_cursor
    let (user_schema_text, position) = with_cursor("type User { id: ID! na|me: String }");
    let user_schema_uri = write_project_file(&dir, "user.graphql", &user_schema_text);
    lsp_did_open(
        &mut service,
        user_schema_uri.clone(),
        "graphql",
        1,
        &user_schema_text,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: user_schema_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations for User.name");

    // Count references in query file
    let query_refs: Vec<_> = locations.iter().filter(|l| l.uri == query_uri).collect();

    // Should only have 1 reference in query file (user { name }), NOT product { name }
    assert!(
        query_refs.len() == 1,
        "Expected exactly 1 reference in query file (User.name), got {}. Product.name should not be included.",
        query_refs.len()
    );

    // Verify the single reference is for user { name }, not product { name }
    // In "query { user { name } product { name } }", user.name is at char ~17
    let ref_loc = &query_refs[0];
    assert!(
        ref_loc.range.start.character < 25,
        "Reference should be on user.name (before char 25), but found at char {}",
        ref_loc.range.start.character
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_references_from_query() {
    // Test finding references when cursor is on a field selection in a query
    let schema_text = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the schema file
    let schema_uri = write_project_file(&dir, "schema.graphql", schema_text);
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    // 2. Create and Open a query file
    let (query_text, position) = with_cursor("query GetUser { user { na|me } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // 3. Trigger Find References on "name" in the query
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
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
    assert!(
        locations.len() >= 2,
        "Expected at least 2 locations (definition + usage), got {}",
        locations.len()
    );

    // Should include the schema definition
    let has_schema_def = locations.iter().any(|l| l.uri == schema_uri);
    assert!(has_schema_def, "Missing schema definition in references");
}
