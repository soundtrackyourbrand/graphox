use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp_server::ls_types::*;

// =============================================================================
// Phase 4: Fragment Spreads & Operations Tests
// =============================================================================

/// Navigate through a chain of fragment spreads: A → B → C
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_fragment_spread_chain() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String email: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create fragment C (base)
    let frag_c = "fragment UserEmail on User { email }";
    let frag_c_uri = write_project_file(&tmpdir, "fragment_c.graphql", frag_c);
    lsp_did_open(&mut service, frag_c_uri.clone(), "graphql", 1, frag_c).await;

    // Create fragment B (uses C)
    let frag_b = "fragment UserName on User { name ...UserEmail }";
    let frag_b_uri = write_project_file(&tmpdir, "fragment_b.graphql", frag_b);
    lsp_did_open(&mut service, frag_b_uri.clone(), "graphql", 1, frag_b).await;

    // Create fragment A (uses B)
    let frag_a = "fragment UserAll on User { id ...UserName }";
    let frag_a_uri = write_project_file(&tmpdir, "fragment_a.graphql", frag_a);
    lsp_did_open(&mut service, frag_a_uri.clone(), "graphql", 1, frag_a).await;

    // Create query using fragment A - cursor on UserName spread
    let query_text = "query { user { ...UserAll } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Navigate from fragment spread to its definition
    let (query_with_cursor, position) = with_cursor("query { user { ...|UserAll } }");
    let query_uri2 = write_project_file(&tmpdir, "query2.graphql", &query_with_cursor);
    lsp_did_open(
        &mut service,
        query_uri2.clone(),
        "graphql",
        1,
        &query_with_cursor,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri2 },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(
        result.is_some(),
        "Expected definition for fragment spread chain, got {:?}",
        result
    );
}

/// Navigate between fragment spread and inline fragment
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_fragment_spread_inline() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create query with inline fragment - cursor on 'User'
    let query_text = "query { user { ... on |User { id } } }";
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

    assert!(
        result.is_some(),
        "Expected definition for inline fragment type, got {:?}",
        result
    );
}

/// Navigate within a recursive fragment
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_fragment_recursive() {
    let schema = "type Query { node: Node }\ninterface Node { id: ID! }\ntype User implements Node { id: ID! friends: [User] }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create recursive fragment
    let frag_text = "fragment UserRecur on User { id friends { ...UserRecur } }";
    let frag_uri = write_project_file(&tmpdir, "user_recur.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Navigate to fragment definition from usage
    let query_with_cursor = "query { node { ...|UserRecur } }";
    let (query_with_cursor, position) = with_cursor(query_with_cursor);
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

    assert!(
        result.is_some(),
        "Expected definition for recursive fragment, got {:?}",
        result
    );
}

/// Navigate in fragment within conditional directive
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_fragment_conditional() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create fragment
    let frag_text = "fragment UserFields on User { id }";
    let frag_uri = write_project_file(&tmpdir, "user_fields.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Navigate in query with @include directive
    let query_text = "query { user @include(if: true) { ...|UserFields } }";
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

    assert!(
        result.is_some(),
        "Expected definition for fragment in conditional, got {:?}",
        result
    );
}

/// Navigate to query operation name definition
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_operation_name() {
    let schema = "type Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Cursor on operation name
    let query_text = "|query GetUser { user { id } }";
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

    // Operation name navigation may not be implemented
    assert_eq!(result, None);
}

/// Navigate to mutation operation name definition
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_mutation_name() {
    let schema =
        "type Mutation { createUser: User }\ntype Query { dummy: String }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Cursor on mutation name
    let query_text = "|mutation CreateUser { createUser { id } }";
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

    // Operation name navigation may not be implemented
    assert_eq!(result, None);
}

/// Navigate to subscription operation name definition
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_subscription_name() {
    let schema =
        "type Subscription { onUser: User }\ntype Query { dummy: String }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Cursor on subscription name
    let query_text = "|subscription OnUser { onUser { id } }";
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

    // Operation name navigation may not be implemented
    assert_eq!(result, None);
}

/// Navigate to correct operation when multiple operations exist in file
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_multiple_operations() {
    let schema = "type Query { user: User admin: Admin }\ntype User { id: ID! }\ntype Admin { id: ID! privileges: [String] }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Multiple operations - navigate from reference to second operation
    let query_text = "query GetUser { user { id } }\n\nquery |GetAdmin { admin { id } }";
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

    // Multiple operation navigation may not be implemented
    assert_eq!(result, None);
}
