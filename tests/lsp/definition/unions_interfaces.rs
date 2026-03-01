use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

// =============================================================================
// Phase 2: Union & Interface Types
// =============================================================================

/// Navigate to type in union from ... on UnionMember
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_union_member() {
    let schema = "union Pet = Dog | Cat\ntype Dog { bark: String }\ntype Cat { meow: String }\ntype Query { pet: Pet }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Navigate to Dog in ... on Dog
    let (query_text, position) = with_cursor("query { pet { ... on |Dog { bark } } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

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
        "Expected definition for Dog union member, got {:?}",
        result
    );
}

/// Navigate to interface from ... on Interface
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_interface_type() {
    let schema = "interface Node { id: ID! }\ntype User implements Node { id: ID! name: String }\ntype Query { node: Node }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Navigate to Node in ... on Node
    let (query_text, position) = with_cursor("query { node { ... on |Node { id } } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

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
        "Expected definition for Node interface, got {:?}",
        result
    );
}

/// Navigate to interface from implements Interface
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_implements() {
    let schema = "interface Node { id: ID! }\ntype User implements Node { id: ID! name: String }\ntype Query { user: User }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Navigate to Node in implements Node (in schema file)
    let (schema_text, position) =
        with_cursor("type User implements |Node { id: ID! name: String }");
    let schema_uri2 = write_project_file(&tmpdir, "schema2.graphql", &schema_text);
    lsp_did_open(
        &mut service,
        schema_uri2.clone(),
        "graphql",
        1,
        &schema_text,
    )
    .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri2 },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // Should navigate to original Node interface definition
    assert!(
        result.is_some(),
        "Expected definition for Node, got {:?}",
        result
    );
}

/// Navigate when union has 3+ types
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_union_multiple_members() {
    let schema = "union Media = Book | Movie | Article\ntype Book { title: String }\ntype Movie { duration: Int }\ntype Article { wordCount: Int }\ntype Query { media: Media }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Navigate to Article in union
    let (query_text, position) = with_cursor("query { media { ... on |Article { wordCount } } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

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
        "Expected definition for Article union member, got {:?}",
        result
    );
}

/// Navigate to field on interface type
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_interface_field() {
    let schema = "interface Node { id: ID! createdAt: String }\ntype User implements Node { id: ID! createdAt: String name: String }\ntype Query { user: User }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Navigate to 'createdAt' field in query that returns User
    let (query_text, position) = with_cursor("query { user { |createdAt } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

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

    // Should navigate to createdAt field definition (which could be on User or Node)
    assert!(
        result.is_some(),
        "Expected definition for createdAt field, got {:?}",
        result
    );
}
