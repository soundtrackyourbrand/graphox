use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_document_highlight_variable_in_operation() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user(id: ID!): User }\ntype User { id: ID! name: String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(Request::build("initialized").params(serde_json::json!({})).finish())
        .await
        .unwrap();

    // Open a GraphQL document with a variable
    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetUser($id: ID!) { user(id: $id) { id name } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // Small delay to ensure document is processed
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Trigger document highlight on $id in the variable definition
    let position = Position::new(0, 15); // Position inside $id variable name (on 'i')
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

    let request = Request::build("textDocument/documentHighlight")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<DocumentHighlight>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let highlights = result.expect("Expected highlights");
    
    // Should highlight both the definition and the usage
    assert_eq!(highlights.len(), 2, "Expected 2 highlights (definition + usage)");

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user(id: ID!): User }\ntype User { id: ID! name: String age: Int }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(Request::build("initialized").params(serde_json::json!({})).finish())
        .await
        .unwrap();

    // Create a file with both fragment and query in the same file
    let query_path = base_dir.join("query_with_fragment.graphql");
    let query_text = r#"fragment UserFields on User { id name @skip(if: $skipName) }

query GetUser($id: ID!, $skipName: Boolean!) { user(id: $id) { ...UserFields } }"#;
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // Small delay to ensure processing completes
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Trigger document highlight on $skipName in the query (line 2, position 25 is 's' in skipName)
    let position = Position::new(2, 25); // Position inside $skipName variable name
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

    let request = Request::build("textDocument/documentHighlight")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<DocumentHighlight>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let highlights = result.expect("Expected highlights");
    
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
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user(id: ID!): User }\ntype User { id: ID! name: String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.{graphql,tsx}".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(Request::build("initialized").params(serde_json::json!({})).finish())
        .await
        .unwrap();

    // Open a TSX file with embedded GraphQL
    let tsx_path = base_dir.join("component.tsx");
    let tsx_text = r#"
import { gql } from '@apollo/client';

const GET_USER = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      name
    }
  }
`;
"#;
    fs::write(&tsx_path, tsx_text).unwrap();
    let tsx_path = std::fs::canonicalize(tsx_path).unwrap();
    let tsx_uri = Url::from_file_path(&tsx_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: tsx_uri.clone(),
            language_id: "typescriptreact".to_string(),
            version: 1,
            text: tsx_text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // Small delay to ensure document is processed
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Trigger document highlight on $id in the variable definition
    // Line 4 is "  query GetUser($id: ID!) {"
    let position = Position::new(4, 17); // Position of $id
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

    let request = Request::build("textDocument/documentHighlight")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<DocumentHighlight>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let highlights = result.expect("Expected highlights");
    
    // Should highlight both the definition and the usage
    assert_eq!(highlights.len(), 2, "Expected 2 highlights (definition + usage)");

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
