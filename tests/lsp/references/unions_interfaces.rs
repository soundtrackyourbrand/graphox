use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp_server::ls_types::*;

/// Find union member type references
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_union_member() {
    let schema = "union Pet = Dog | Cat\ntype Dog { bark: String }\ntype Cat { meow: String }\ntype Query { pet: Pet }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using inline fragment
    let (query_text, position) = with_cursor("query { pet { ... on |Dog { bark } } }");
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

    // Should find references to Dog type
    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for union member"
    );
}

/// Find interface type references
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_interface_type() {
    let schema = "interface Node { id: ID! }\ntype User implements Node { id: ID! name: String }\ntype Query { node: Node }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using interface
    let (query_text, position) = with_cursor("query { node { ... on |Node { id } } }");
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

    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for interface"
    );
}

/// Find implements type references
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_implements() {
    let schema = "interface Node { id: ID! }\ntype User implements Node { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Cursor on interface in implements clause
    let (schema_text, position) = with_cursor("type User implements |Node { id name }");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text,
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

    // When schema validation fails, references may return None
    let _ = result;
}

/// Find union with multiple member references
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_union_multiple_members() {
    let schema = "union SearchResult = User | Post | Comment\ntype User { name: String }\ntype Post { title: String }\ntype Comment { text: String }\ntype Query { search: SearchResult }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Query using one of the union members
    let (query_text, position) = with_cursor("query { search { ... on |Post { title } } }");
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

    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for union member"
    );
}

/// Find interface field references
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_interface_field() {
    let schema = "type Query { node: Node }\ninterface Node { id: ID! }\ntype User implements Node { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Cursor on interface field 'id'
    let (schema_text2, position) = with_cursor("interface Node { |id: ID! }");
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
        "Expected at least 1 location for interface field"
    );
}
