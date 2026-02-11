use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_document_highlight_variable_in_operation() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    // Ensure options match previous test
    config.enable_schema_cache = Some(true);
    config.lsp_automatic_codegen = Some(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open a GraphQL document with a variable
    let (query_text, position) =
        with_cursor("query GetUser($i|d: ID!) { user(id: $id) { id name } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // Small delay to ensure document is processed
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Trigger document highlight on $id in the variable definition
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight both the definition and the usage
    assert_eq!(
        highlights.len(),
        2,
        "Expected 2 highlights (definition + usage)"
    );

    // Check that we have one WRITE (definition) and one READ (usage)
    let write_count = highlights
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
        .count();
    let read_count = highlights
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
        .count();

    assert_eq!(write_count, 1, "Expected 1 WRITE highlight (definition)");
    assert_eq!(read_count, 1, "Expected 1 READ highlight (usage)");
}

#[tokio::test]
async fn test_document_highlight_variable_across_fragments_same_file() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! name: String age: Int }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config.enable_schema_cache = Some(true);
    config.lsp_automatic_codegen = Some(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a file with both fragment and query in the same file
    let query_text = r#"fragment UserFields on User { id name @skip(if: $skipName) }

query GetUser($id: ID!, $skip|Name: Boolean!) { user(id: $id) { ...UserFields } }"#;
    let (query_text, position) = with_cursor(query_text);
    let query_uri = write_project_file(&dir, "query_with_fragment.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // Small delay to ensure processing completes
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Trigger document highlight on $skipName in the query
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight the definition in query and usage in fragment (same file)
    assert!(
        highlights.len() >= 2,
        "Expected at least 2 highlights (definition in query + usage in fragment), got {}",
        highlights.len()
    );

    // Check that we have one WRITE (definition in query)
    let write_count = highlights
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
        .count();

    assert_eq!(
        write_count, 1,
        "Expected 1 WRITE highlight (definition in query)"
    );

    // Check that we have at least one READ (usage in fragment)
    let read_count = highlights
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
        .count();

    assert!(
        read_count >= 1,
        "Expected at least 1 READ highlight (usage in fragment)"
    );
}

#[tokio::test]
async fn test_document_highlight_variable_in_tsx() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config.enable_schema_cache = Some(true);
    config.lsp_automatic_codegen = Some(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open a TSX file with embedded GraphQL
    let (tsx_text, position) = with_cursor(
        r#"
import { gql } from '@apollo/client';

const GET_USER = gql`
  query GetUser($i|d: ID!) {
    user(id: $id) {
      id
      name
    }
  }
`;
"#,
    );
    let tsx_uri = write_project_file(&dir, "component.tsx", &tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        &tsx_text,
    )
    .await;

    // Small delay to ensure document is processed
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Trigger document highlight on $id in the variable definition
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight both the definition and the usage
    assert_eq!(
        highlights.len(),
        2,
        "Expected 2 highlights (definition + usage)"
    );

    // Check that we have one WRITE (definition) and one READ (usage)
    let write_count = highlights
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
        .count();
    let read_count = highlights
        .iter()
        .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
        .count();

    assert_eq!(write_count, 1, "Expected 1 WRITE highlight (definition)");
    assert_eq!(read_count, 1, "Expected 1 READ highlight (usage)");
}
