use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp_server::ls_types::*;

/// Find field references within schema
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_schema_field_to_field() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create another file that uses the field
    let query_text = "query { user { name } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references on field in schema
    let (schema_text2, position) = with_cursor("type User { id: ID! na|me: String }");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text2);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text2,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri2 },
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
        !locations.is_empty(),
        "Expected at least 1 location for schema field"
    );
}

/// Find type references within schema
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_schema_type_to_type() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query that uses User type
    let query_text = "query { user { id } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references on type in schema
    let (schema_text2, position) = with_cursor("type |User { id: ID! }");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text2);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text2,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri2 },
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
        !locations.is_empty(),
        "Expected at least 1 location for schema type"
    );
}

/// Find enum value references in schema
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_schema_enum_value() {
    let schema = "enum Status { ACTIVE INACTIVE }\ntype Query { status: Status }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using enum value
    let query_text = "query { status }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references on enum value in schema
    let (schema_text2, position) = with_cursor("enum Status { ACT|IVE INACTIVE }");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text2);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text2,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri2 },
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
        !locations.is_empty(),
        "Expected at least 1 location for enum value"
    );
}

/// Find argument references in schema field definitions
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_schema_argument() {
    let schema = "type Query { user(id: ID!): User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using the argument
    let query_text = "query { user(id: \"1\") { id } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references on argument in schema
    let (schema_text2, position) = with_cursor("type Query { user(|id: ID!): User }");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text2);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text2,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri2 },
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
        !locations.is_empty(),
        "Expected at least 1 location for argument"
    );
}

/// Find directive references in schema
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_schema_directive() {
    let schema = "directive @skip(if: Boolean!) on FIELD_DEFINITION\ntype Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using directive
    let query_text = "query { user @skip(if: false) { id } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references on directive definition
    let (schema_text2, position) =
        with_cursor("directive @|skip(if: Boolean!) on FIELD_DEFINITION");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text2);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text2,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri2 },
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
        "Expected at least 2 locations (definition + usage)"
    );
}
