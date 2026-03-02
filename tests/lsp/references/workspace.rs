use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

// =============================================================================
// Phase 7: Workspace Features
// =============================================================================

/// Find references across multiple files in workspace
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_workspace_wide() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Multiple query files using the same field
    let query1_text = "query { user { name } }";
    let query1_uri = write_project_file(&tmpdir, "query1.graphql", query1_text);
    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;

    let query2_text = "query { user { name } }";
    let query2_uri = write_project_file(&tmpdir, "query2.graphql", query2_text);
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Find references on schema field 'name'
    let (user_schema, position) = with_cursor("type User { id: ID! na|me: String }");
    let user_schema_uri = write_project_file(&tmpdir, "user.graphql", &user_schema);
    lsp_did_open(
        &mut service,
        user_schema_uri.clone(),
        "graphql",
        1,
        &user_schema,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: user_schema_uri,
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
    // Should find references across all files (definition + 2 usages)
    assert!(
        locations.len() >= 3,
        "Expected at least 3 locations (definition + 2 usages), got {}",
        locations.len()
    );
}

/// Find variable references through fragment chain (transitive)
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_transitive_fragments() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Fragment using variable
    let frag_text = "fragment UserFrag on User { name(id: $id) }";
    let frag_uri = write_project_file(&tmpdir, "frag.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Query defining variable and using fragment
    let query_text = "query GetUser($id: ID!) { user { ...UserFrag } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references on $id variable definition in query
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position: Position::new(0, 15), // Position of $id in query
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Transitive fragment variable references may not be fully supported
    assert!(
        result.as_ref().map(|l| !l.is_empty()).unwrap_or(false),
        "Expected Some(locations) but got {:?}",
        result
    );
}

/// Test with include_declaration: false
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_exclude_declaration() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Fragment definition
    let frag_text = "fragment UserFields on User { id name }";
    let frag_uri = write_project_file(&tmpdir, "fragment.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Query using fragment
    let query_text = "query { user { ...UserFields } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Find references with include_declaration: false
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: frag_uri },
            position: Position::new(0, 9), // Position of "UserFields"
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: false,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    // Should NOT include declaration, so only the spread
    assert_eq!(
        locations.len(),
        1,
        "Expected exactly 1 location (spread, no def)"
    );
    assert_eq!(locations[0].uri, query_uri);
}

/// Test partial result handling (large result set)
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_partial_result() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create many files using the same field
    for i in 0..20 {
        let query_text = format!("query Q{} {{ user {{ id }} }}", i);
        let filename = format!("query{}.graphql", i);
        let query_uri = write_project_file(&tmpdir, &filename, &query_text);
        lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;
    }

    // Find references on id field in schema
    let (user_schema, position) = with_cursor("type User { |id: ID! }");
    let user_schema_uri = write_project_file(&tmpdir, "user.graphql", &user_schema);
    lsp_did_open(
        &mut service,
        user_schema_uri.clone(),
        "graphql",
        1,
        &user_schema,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: user_schema_uri,
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
    // Should find all references across many files
    assert!(
        locations.len() >= 20,
        "Expected at least 20 locations, got {}",
        locations.len()
    );
}

/// Performance test - many references
#[tokio::test]
#[ntest::timeout(10000)]
async fn test_references_performance() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create 100 files using the id field
    for i in 0..100 {
        let query_text = format!("query Q{} {{ user {{ id }} }}", i);
        let filename = format!("query{}.graphql", i);
        let query_uri = write_project_file(&tmpdir, &filename, &query_text);
        lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;
    }

    // Find references on id field in schema
    let (user_schema, position) = with_cursor("type User { |id: ID! }");
    let user_schema_uri = write_project_file(&tmpdir, "user.graphql", &user_schema);
    lsp_did_open(
        &mut service,
        user_schema_uri.clone(),
        "graphql",
        1,
        &user_schema,
    )
    .await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: user_schema_uri,
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let start = std::time::Instant::now();
    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;
    let elapsed = start.elapsed();

    let locations = result.expect("Expected locations");
    assert!(
        locations.len() >= 100,
        "Expected at least 100 locations, got {}",
        locations.len()
    );

    // Should complete in reasonable time
    assert!(
        elapsed.as_millis() < 5000,
        "References took too long: {}ms",
        elapsed.as_millis()
    );
}
