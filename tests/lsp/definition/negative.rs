use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

// =============================================================================
// Phase 5: Negative Tests
// =============================================================================

/// Unknown field returns None
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_unknown_field() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query with unknown field
    let query_text = "query { |unknownField { id } }";
    let (query_with_cursor, position) = with_cursor(query_text);
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_with_cursor);
    lsp_did_open(
        &mut service,
        query_uri.clone(),
        "graphql",
        1,
        &query_with_cursor,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // Unknown field should return None
    assert!(
        result.is_none(),
        "Expected None for unknown field, got {:?}",
        result
    );
}

/// Unknown type returns None
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_unknown_type() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query with unknown type
    let query_text = "query { user: |UnknownType }";
    let (query_with_cursor, position) = with_cursor(query_text);
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_with_cursor);
    lsp_did_open(
        &mut service,
        query_uri.clone(),
        "graphql",
        1,
        &query_with_cursor,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // Unknown type should return None
    assert!(
        result.is_none(),
        "Expected None for unknown type, got {:?}",
        result
    );
}

/// Unknown fragment returns None
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_fragment_not_found() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query with non-existent fragment
    let query_text = "query { user { ...|NonExistent } }";
    let (query_with_cursor, position) = with_cursor(query_text);
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_with_cursor);
    lsp_did_open(
        &mut service,
        query_uri.clone(),
        "graphql",
        1,
        &query_with_cursor,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // Unknown fragment should return None
    assert!(
        result.is_none(),
        "Expected None for non-existent fragment, got {:?}",
        result
    );
}

/// Undefined variable returns None
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_variable_undefined() {
    let schema = "type Query { user(id: ID!): User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using undefined variable
    let query_text = "query { user(id: $|undeclared) { id } }";
    let (query_with_cursor, position) = with_cursor(query_text);
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_with_cursor);
    lsp_did_open(
        &mut service,
        query_uri.clone(),
        "graphql",
        1,
        &query_with_cursor,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // Undefined variable should return None
    assert!(
        result.is_none(),
        "Expected None for undefined variable, got {:?}",
        result
    );
}

/// Outside gql tag returns None
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_outside_gql_tag() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // TypeScript code outside gql tag
    let tsx_text = r"const |userName = 'test';
const query = graphql\`{ user { id } }\`;";
    let (tsx_with_cursor, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "test.tsx", &tsx_with_cursor);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescript",
        1,
        &tsx_with_cursor,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // TypeScript code outside gql tag should return None
    assert!(
        result.is_none(),
        "Expected None for TS code outside gql tag, got {:?}",
        result
    );
}

/// Invalid position returns None/error
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_invalid_position() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Open query file
    let query_text = "query { user { id } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Position beyond document end
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position: Position {
                line: 100,
                character: 100,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // Invalid position should return None
    assert!(
        result.is_none(),
        "Expected None for invalid position, got {:?}",
        result
    );
}
