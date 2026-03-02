use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

/// References for unknown field should return empty
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_unknown_field() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query with unknown field
    let (query_text, position) = with_cursor("query { un|knownField }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
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

    // Unknown fields should return None or empty
    assert!(
        result.as_ref().map(|v| v.is_empty()).unwrap_or(true),
        "Expected None or empty for unknown field, got: {:?}",
        result
    );
}

/// References for unknown type should return empty
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_unknown_type() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query with unknown type
    let (query_text, position) = with_cursor("query { user: |UnknownType }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
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

    // Expected None or empty
    assert!(
        result.as_ref().map(|v| v.is_empty()).unwrap_or(true),
        "Expected None or empty, got: {:?}",
        result
    );
}

/// References for non-existent fragment should return empty
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_fragment_not_found() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query with non-existent fragment spread
    let (query_text, position) = with_cursor("query { user { ...Non|Existent } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
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

    // Expected None or empty
    assert!(
        result.as_ref().map(|v| v.is_empty()).unwrap_or(true),
        "Expected None or empty, got: {:?}",
        result
    );
}

/// References for undefined variable should return empty
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_variable_undefined() {
    let schema = "type Query { user(id: ID!): User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using undeclared variable
    let (query_text, position) = with_cursor("query { user(id: $|undeclared) }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
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

    // Expected None or empty
    assert!(
        result.as_ref().map(|v| v.is_empty()).unwrap_or(true),
        "Expected None or empty, got: {:?}",
        result
    );
}

/// References outside gql tag should return empty
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_outside_gql_tag() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.tsx");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // TSX file with cursor on TypeScript code (not in gql tag)
    let tsx_text = r#"const x = "hello";"#;
    let tsx_uri = write_project_file(&tmpdir, "test.tsx", tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position: Position::new(0, 6), // Position of "hello"
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should return empty or None for TypeScript code
    assert!(result.as_ref().map(|v| v.is_empty()).unwrap_or(true));
}

/// References with invalid position beyond document
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_invalid_position() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query file with content
    let query_text = "query { user { id } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Position beyond document
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position: Position::new(100, 100), // Way beyond document
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should return empty or None for invalid position
    assert!(
        result.as_ref().map(|v| v.is_empty()).unwrap_or(true),
        "Expected None or empty for invalid position, got: {:?}",
        result
    );
}
