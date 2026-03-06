use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_variable_in_operation() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    // Ensure options match previous test
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

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
#[ntest::timeout(3000)]
async fn test_document_highlight_variable_across_fragments_same_file() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! name: String age: Int }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

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
#[ntest::timeout(3000)]
async fn test_document_highlight_variable_in_tsx() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

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

// Phase 1: Variable/Fragment Scenarios

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_variable_multiple_usages() {
    let schema =
        "type Query { user(id: ID!, name: String): User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Variable used 3 times: definition + 2 usages
    let (query_text, position) =
        with_cursor("query Test($i|d: ID!) { user(id: $id) { id } user(id: $id) { name } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

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

    // Should have 3 highlights: definition + 2 usages
    assert_eq!(
        highlights.len(),
        3,
        "Expected 3 highlights (definition + 2 usages)"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_fragment_cross_file() {
    let schema = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Fragment in one file
    let frag_text = "fragment UserFields on User { id name }";
    let frag_uri = write_project_file(&dir, "fragment.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Query using fragment in another file
    let query_text = "query { user { ...UserFields } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Highlight in fragment file at the fragment name
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: Position::new(0, 9), // position of "U" in "UserFields"
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight at least the fragment definition
    assert!(!highlights.is_empty(), "Expected at least 1 highlight");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_named_operation() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Two queries with same name - should highlight both
    let query_text = r#"
query GetUser {
    user { id }
}

query GetUser {
    user { id }
}
"#;
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Position at "GetUser" on line 2 (1-indexed), position 7
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position::new(1, 7), // "query GetUser" - first occurrence
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight both occurrences of GetUser
    assert!(
        highlights.len() >= 2,
        "Expected at least 2 highlights for named operation"
    );
}

// Phase 2: Edge Cases

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_anonymous_operation() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { user { id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Position on "query" keyword - anonymous ops shouldn't be highlightable
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position::new(0, 0),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    // Anonymous operations should return None
    assert!(highlights.is_none() || highlights.as_ref().unwrap().is_empty());
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_unused_variable() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Unused variable - defined but never used
    let (query_text, position) =
        with_cursor("query GetUser($|unused: String!) { user(id: \"1\") { id } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

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

    // Should have 1 highlight (the definition) even though unused
    assert_eq!(
        highlights.len(),
        1,
        "Expected 1 highlight for unused variable definition"
    );
}

// Phase 3: TSX Completeness

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_fragment_tsx() {
    let schema = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.tsx");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Test with a query name (similar to test_document_highlight_operation_tsx but different name position)
    let tsx_text = r#"
const q = gql`
  query GetUserFields {
    user { id name }
  }
`;
"#;
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Highlight in TSX file at the query name
    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
            },
            position: Position::new(2, 8), // position of "G" in "GetUserFields" on line 2
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight the query name
    assert!(!highlights.is_empty());
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_operation_tsx() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.tsx");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let tsx_text = r#"
const q = gql`
  query GetUser {
    user { id }
  }
`;
"#;
    let tsx_uri = write_project_file(&dir, "Component.tsx", tsx_text);
    lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
            },
            position: Position::new(2, 8), // position of "GetUser" in "query GetUser"
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights: Option<Vec<DocumentHighlight>> =
        lsp_request_typed(&mut service, "textDocument/documentHighlight", &params).await;
    let highlights = highlights.expect("Expected highlights");

    // Should highlight the named operation
    assert!(!highlights.is_empty());
}

// Phase 4: Negative Tests

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_field() {
    let schema = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (query_text, position) = with_cursor("query { user { |id } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

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
    // Fields are not supported - should return None
    assert!(highlights.is_none() || highlights.as_ref().unwrap().is_empty());
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_type() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Position on "User" type in schema
    let (query_text, position) = with_cursor("query { user: |User }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

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
    // Types are not supported - should return None
    assert!(highlights.is_none() || highlights.as_ref().unwrap().is_empty());
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_document_highlight_literal() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_enable_schema_cache(true);
    config = config.with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Position on string literal
    let (query_text, position) = with_cursor("query { user(id: \"|123\") { id } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

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
    // Literals are not supported - should return None
    assert!(highlights.is_none() || highlights.as_ref().unwrap().is_empty());
}
