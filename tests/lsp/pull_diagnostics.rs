use crate::support::{
    create_lsp_service_with_socket, create_service, lsp_did_open, lsp_request_typed,
};
use futures_util::StreamExt;
use graphox::{
    Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_basic() {
    let query_text = "query GetUser { user { nonExistentField } }"; // Invalid field
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query_path = base_dir.join("query.graphql");

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = create_lsp_service_with_socket(config);

    // Track push diagnostics (should not receive any when using pull)
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_push_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    // Initialize with pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result: InitializeResult =
        lsp_request_typed(&mut service, "initialize", &init_params).await;
    assert!(result.capabilities.diagnostic_provider.is_some());

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Request diagnostics via pull
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    // Should return full diagnostic report
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            assert!(
                !report.full_document_diagnostic_report.items.is_empty(),
                "Should have diagnostics for invalid field"
            );
            assert!(
                report.full_document_diagnostic_report.result_id.is_some(),
                "Should have result_id"
            );

            // Verify diagnostic mentions the field
            let diag_msg = &report.full_document_diagnostic_report.items[0].message;
            assert!(diag_msg.contains("nonExistentField") || diag_msg.contains("field"));
        }
        _ => panic!("Expected full diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_unchanged() {
    let query_text = "query GetUser { user { id name } }"; // Valid query
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query_path = base_dir.join("query.graphql");

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, _handle) = create_service(config);

    // Initialize with pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // First pull request
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first_result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    let result_id = match first_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.result_id.clone()
        }
        _ => panic!("Expected full diagnostic report on first request"),
    };

    // Second pull request with same result_id - document unchanged
    let diag_params2 = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: result_id,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let second_result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params2).await;

    // Should return unchanged report
    match second_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => {
            // Success - document unchanged
        }
        _ => panic!("Expected unchanged diagnostic report when document hasn't changed"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics() {
    let query1_text = "query GetUser { user { id } }";
    let query2_text = "query GetPost { post { title } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User post: Post } type User { id: ID! } type Post { title: String }",
        )
        .with_file("query1.graphql", query1_text)
        .with_file("query2.graphql", query2_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query1_path = base_dir.join("query1.graphql");
    let query2_path = base_dir.join("query2.graphql");

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, _handle) = create_service(config);

    // Initialize with pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open both documents
    let query1_uri = Url::from_file_path(&query1_path).unwrap();
    let query2_uri = Url::from_file_path(&query2_path).unwrap();

    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Request workspace diagnostics
    let workspace_diag_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: vec![],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &workspace_diag_params).await;

    match result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            // Should have diagnostic reports for both documents
            assert!(
                !report.items.is_empty(),
                "Should have workspace diagnostic items"
            );

            // Check that we have reports for our documents
            let mut found_query1 = false;
            let mut found_query2 = false;

            for item in &report.items {
                if let WorkspaceDocumentDiagnosticReport::Full(full_report) = item {
                    if full_report.uri == query1_uri {
                        found_query1 = true;
                    }
                    if full_report.uri == query2_uri {
                        found_query2 = true;
                    }
                }
            }

            assert!(
                found_query1 || found_query2,
                "Should have diagnostics for at least one query"
            );
        }
        _ => panic!("Expected workspace diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_fallback_to_push_diagnostics() {
    let query_text = "query GetUser { user { invalidField } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query_path = base_dir.join("query.graphql");

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = create_lsp_service_with_socket(config);

    // Track push diagnostics
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_push_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    // Initialize WITHOUT pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                // No diagnostic capability
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Wait for validation (poll for diagnostics)
    let start = tokio::time::Instant::now();
    loop {
        {
            let push_diags = received_push_diags.lock().unwrap();
            if !push_diags.is_empty() {
                break;
            }
        }
        if start.elapsed() > Duration::from_secs(2) {
            panic!("Timed out waiting for push diagnostics");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Verify push diagnostics WERE sent (fallback behavior)
    let push_diags = received_push_diags.lock().unwrap();
    assert!(
        !push_diags.is_empty(),
        "Should receive push diagnostics when client doesn't support pull"
    );

    let last_diag = push_diags.last().unwrap();
    assert_eq!(last_diag["uri"].as_str().unwrap(), query_uri.as_str());
    assert!(
        !last_diag["diagnostics"].as_array().unwrap().is_empty(),
        "Should have diagnostics for invalid field"
    );
}
