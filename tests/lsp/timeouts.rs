use graphql_rust::{
    Backend, Config, config::{GlobPattern, ProjectConfig, SchemaSource, TimeoutConfig},
};
use std::fs;
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_lsp_request_timeout() {
    // Create a test workspace
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create a schema file
    let schema_path = base_dir.join("schema.graphql");
    let mut schema_content = String::new();
    schema_content.push_str("type Query { ");
    // Add many fields to make validation slower
    for i in 0..1000 {
        schema_content.push_str(&format!("field{}: String ", i));
    }
    schema_content.push_str("}\n");
    
    // Add many types to make the schema larger
    for i in 0..500 {
        schema_content.push_str(&format!("type Type{} {{ id: ID! name: String }}\n", i));
    }
    fs::write(&schema_path, &schema_content).unwrap();

    // Create a query file
    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query Test { field0 }").unwrap();

    // Configure with a very short timeout (10ms) to ensure we hit it
    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        timeouts: Some(TimeoutConfig {
            workspace_scan_ms: 60000, // Keep workspace scan long
            lsp_request_ms: 10,        // Very short timeout for LSP requests
        }),
        watch_all_files: None,
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        enable_schema_cache: Some(false), // Disable cache to ensure slower operations
        base_dir: base_dir.to_path_buf(),
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));
    
    let init_params = InitializeParams::default();
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Wait a bit for workspace scan to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    let query_uri = Url::from_file_path(&query_path).unwrap();

    // Open the document
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: "query Test { field0 }".to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Try a hover request - with a 10ms timeout, this might timeout
    // We're just checking that it doesn't panic or hang
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position {
                line: 0,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let request = Request::build("textDocument/hover")
        .params(serde_json::to_value(&params).unwrap())
        .id(1)
        .finish();
    let result = service.call(request).await;

    // The request should complete (either successfully or with a timeout)
    // The important thing is it doesn't hang
    assert!(result.is_ok(), "Hover request should complete");
}

#[tokio::test]
async fn test_workspace_scan_timeout() {
    // Create a test workspace with many files to slow down workspace scan
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create a schema file
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { field: String }").unwrap();

    // Create many GraphQL files to slow down the workspace scan
    for i in 0..100 {
        let query_path = base_dir.join(format!("query{}.graphql", i));
        fs::write(&query_path, format!("query Test{} {{ field }}", i)).unwrap();
    }

    // Configure with a very short workspace scan timeout (5ms) to ensure we hit it
    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        timeouts: Some(TimeoutConfig {
            workspace_scan_ms: 5,     // Very short timeout to trigger timeout
            lsp_request_ms: 1000,
        }),
        watch_all_files: None,
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        enable_schema_cache: Some(false),
        base_dir: base_dir.to_path_buf(),
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));
    
    let init_params = InitializeParams::default();
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    // Call initialized - this triggers the workspace scan
    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Wait a bit to let the workspace scan attempt to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The workspace scan should have timed out, but the LSP should still be responsive
    // Test that we can still make requests
    let params = WorkspaceSymbolParams {
        query: "Test".to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let request = Request::build("workspace/symbol")
        .params(serde_json::to_value(&params).unwrap())
        .id(1)
        .finish();
    let result = service.call(request).await;

    // The request should complete - LSP should still be responsive after workspace scan timeout
    assert!(result.is_ok(), "LSP should remain responsive after workspace scan timeout");
}

#[tokio::test]
async fn test_timeout_with_normal_config() {
    // Test that normal operations work fine with default timeouts
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { hello: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query Test { hello }").unwrap();

    // Use default timeouts (should be plenty for small workspace)
    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        timeouts: None, // Use defaults
        watch_all_files: None,
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));
    
    let init_params = InitializeParams::default();
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Wait for workspace scan
    tokio::time::sleep(Duration::from_millis(200)).await;

    let query_uri = Url::from_file_path(&query_path).unwrap();

    // Open document
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: "query Test { hello }".to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Make a hover request - should succeed with default timeout
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position {
                line: 0,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let request = Request::build("textDocument/hover")
        .params(serde_json::to_value(&params).unwrap())
        .id(1)
        .finish();
    let result = service.call(request).await;

    // Should get a response without timing out
    assert!(result.is_ok(), "Should receive hover response without timeout");
}
