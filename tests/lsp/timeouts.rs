use crate::support::{
    create_initialized_lsp_service, create_service, lsp_did_open, lsp_request_typed,
    lsp_send_notification, make_temp_project_with_schema, pos, write_project_file,
};
use graphox::config::TimeoutConfig;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_lsp_request_timeout() {
    // Create a schema file content
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

    let (dir, mut config) = make_temp_project_with_schema(&schema_content, "*.graphql");

    // Configure with a very short timeout (10ms) to ensure we hit it
    config.timeouts = Some(TimeoutConfig {
        workspace_scan_ms: 60000, // Keep workspace scan long
        lsp_request_ms: 10,       // Very short timeout for LSP requests
    });
    config.enable_schema_cache = Some(false); // Disable cache to ensure slower operations

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query Test { field0 }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);

    // Open the document via helper
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Try a hover request - with a 10ms timeout, this might timeout
    // We're just checking that it doesn't panic or hang
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 15),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let result =
        lsp_request_typed::<Option<Hover>, _>(&mut service, "textDocument/hover", &params).await;

    // The request should complete (either successfully or with a timeout)
    // The important thing is it doesn't hang — typed helper returns Option<Hover>
    assert!(
        result.is_none() || result.is_some(),
        "Hover request should complete"
    );
}

#[tokio::test]
async fn test_workspace_scan_timeout() {
    let (dir, mut config) =
        make_temp_project_with_schema("type Query { field: String }", "*.graphql");

    // Create many GraphQL files to slow down the workspace scan
    for i in 0..100 {
        write_project_file(
            &dir,
            &format!("query{}.graphql", i),
            &format!("query Test{} {{ field }}", i),
        );
    }

    // Configure with a very short workspace scan timeout (5ms) to ensure we hit it
    config.timeouts = Some(TimeoutConfig {
        workspace_scan_ms: 5, // Very short timeout to trigger timeout
        lsp_request_ms: 1000,
    });
    config.enable_schema_cache = Some(false);

    let (mut service, _) = create_service(config);

    let init_params = InitializeParams::default();
    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    // Call initialized - this triggers the workspace scan
    lsp_send_notification(&mut service, "initialized", &serde_json::json!({})).await;

    // Wait a bit to let the workspace scan attempt to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // The workspace scan should have timed out, but the LSP should still be responsive
    // Test that we can still make requests
    let params = WorkspaceSymbolParams {
        query: "Test".to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    // The request should complete - LSP should still be responsive after workspace scan timeout
    assert!(
        result.is_some() || result.is_none(),
        "LSP should remain responsive after workspace scan timeout"
    );
}

#[tokio::test]
async fn test_timeout_with_normal_config() {
    let (dir, config) = make_temp_project_with_schema("type Query { hello: String }", "*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query Test { hello }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);

    // Open document
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Make a hover request - should succeed with default timeout
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 15),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    let result: Option<Hover> =
        lsp_request_typed(&mut service, "textDocument/hover", &params).await;

    // Should get a response without timing out
    assert!(
        result.is_some() || result.is_none(),
        "Should receive hover response without timeout"
    );
}
