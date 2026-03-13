use crate::support::{
    create_lsp_service_with_socket, create_service, lsp_did_open, lsp_request_typed,
};
use futures_util::StreamExt;
use graphox::{
    Config,
    config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource},
};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp::lsp_types::*;
use tower_service::Service;

async fn wait_for_workspace_loaded(
    service: &mut tower_lsp::LspService<crate::support::LspBackend>,
) {
    let backend = service.inner();
    let start = tokio::time::Instant::now();
    while !backend.workspace_loaded.load(Ordering::SeqCst) {
        assert!(
            start.elapsed() <= Duration::from_secs(10),
            "Timed out waiting for workspace scan to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

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

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

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

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

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
async fn test_pull_diagnostics_refreshes_when_workspace_epoch_changes() {
    let query_text = "query GetUser { user { id name } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let query_path = base_dir.join("query.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

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

    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first_result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    let previous_result_id = match first_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => report
            .full_document_diagnostic_report
            .result_id
            .expect("Expected initial result_id"),
        _ => panic!("Expected full diagnostic report on first request"),
    };

    let backend = service.inner();
    backend
        .last_full_validation_version
        .store(99, Ordering::SeqCst);

    let second_result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(query_uri),
            identifier: None,
            previous_result_id: Some(previous_result_id.clone()),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match second_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            let refreshed_result_id = report
                .full_document_diagnostic_report
                .result_id
                .expect("Expected refreshed result_id");
            assert_ne!(refreshed_result_id, previous_result_id);
            assert!(
                refreshed_result_id.ends_with(":99"),
                "Expected workspace epoch in result_id, got {refreshed_result_id}"
            );
        }
        _ => panic!("Expected full diagnostic report after workspace epoch change"),
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

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

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

    wait_for_workspace_loaded(&mut service).await;

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
async fn test_workspace_diagnostics_refresh_when_workspace_epoch_changes() {
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

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

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

    wait_for_workspace_loaded(&mut service).await;

    let first_result: WorkspaceDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "workspace/diagnostic",
        &WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: vec![],
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    let previous_result_ids = match first_result {
        WorkspaceDiagnosticReportResult::Report(report) => report
            .items
            .into_iter()
            .filter_map(|item| match item {
                WorkspaceDocumentDiagnosticReport::Full(full_report) => full_report
                    .full_document_diagnostic_report
                    .result_id
                    .map(|result_id| PreviousResultId {
                        uri: full_report.uri,
                        value: result_id,
                    }),
                WorkspaceDocumentDiagnosticReport::Unchanged(_) => None,
            })
            .collect::<Vec<_>>(),
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    };

    let backend = service.inner();
    backend
        .last_full_validation_version
        .store(42, Ordering::SeqCst);

    let refreshed_result: WorkspaceDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "workspace/diagnostic",
        &WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match refreshed_result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                !report.items.is_empty(),
                "Expected workspace diagnostics to refresh after the workspace epoch changed"
            );
            for item in report.items {
                match item {
                    WorkspaceDocumentDiagnosticReport::Full(full_report) => {
                        let result_id = full_report
                            .full_document_diagnostic_report
                            .result_id
                            .expect("Expected refreshed workspace result_id");
                        assert!(
                            result_id.ends_with(":42"),
                            "Expected workspace epoch in result_id, got {result_id}"
                        );
                    }
                    WorkspaceDocumentDiagnosticReport::Unchanged(_) => {
                        panic!("Expected stale workspace diagnostics to be recomputed")
                    }
                }
            }
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_omits_unchanged_items() {
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

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

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

    wait_for_workspace_loaded(&mut service).await;

    let first_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: vec![],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first_result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &first_params).await;

    let previous_result_ids = match first_result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                !report.items.is_empty(),
                "Expected initial workspace diagnostics"
            );
            report
                .items
                .into_iter()
                .filter_map(|item| match item {
                    WorkspaceDocumentDiagnosticReport::Full(full_report) => full_report
                        .full_document_diagnostic_report
                        .result_id
                        .map(|result_id| PreviousResultId {
                            uri: full_report.uri,
                            value: result_id,
                        }),
                    WorkspaceDocumentDiagnosticReport::Unchanged(_) => None,
                })
                .collect::<Vec<_>>()
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    };

    let second_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let second_result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &second_params).await;

    match second_result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                report.items.is_empty(),
                "Expected unchanged workspace diagnostics to be omitted"
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_returns_empty_while_workspace_loading() {
    let query_text = "query GetUser { user { id } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

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

    wait_for_workspace_loaded(&mut service).await;

    let backend = service.inner();
    backend.workspace_loaded.store(false, Ordering::SeqCst);

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
            assert!(
                report.items.is_empty(),
                "Expected workspace diagnostics to stay empty while the background scan is loading"
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
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

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

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

    let query_diag = push_diags
        .iter()
        .find(|d| d["uri"].as_str().unwrap() == query_uri.as_str())
        .expect("Should have diagnostics for query.graphql");

    assert!(
        !query_diag["diagnostics"].as_array().unwrap().is_empty(),
        "Should have diagnostics for invalid field"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_refresh_after_config_reload_restores_fragment_context() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String! }",
        )
        .with_file("query.graphql", "query GetUser { user { ...UserFields } }")
        .with_file(
            "fragments.graphql",
            "fragment UserFields on User { id name }",
        )
        .with_file(
            "graphox.yaml",
            r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
    codegen: false
rules:
  required_fields:
    id: true
"#,
        );

    let base_dir = scenario.write_files().unwrap();
    let query_path = base_dir.join("query.graphql");
    let config_path = base_dir.join("graphox.yaml");

    let config = Config::load_from_dir(&base_dir).unwrap().unwrap();
    let (mut service, mut messages) = create_lsp_service_with_socket(config);
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(4);

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("window/logMessage") {
                let params_json = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let params: LogMessageParams = serde_json::from_value(params_json).unwrap();
                if params.message.starts_with("Workspace scan complete") {
                    let _ = scan_done_tx.send(()).await;
                }
            }
        }
    });

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

    tokio::time::timeout(Duration::from_millis(2000), scan_done_rx.recv())
        .await
        .expect("Initial workspace scan did not complete in time")
        .expect("scan_done_rx closed before initial scan completed");

    let query_uri = Url::from_file_path(&query_path).unwrap();
    let query_text = std::fs::read_to_string(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let before_reload: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;
    let before_items = match before_reload {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.items
        }
        _ => panic!("Expected full diagnostic report before reload"),
    };
    assert!(
        before_items.iter().all(|diag| {
            diag.code != Some(NumberOrString::String("required_field_missing".to_string()))
        }),
        "Fragment selections should satisfy required fields before config reload: {before_items:#?}"
    );

    std::fs::write(
        &config_path,
        r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
    codegen: false
rules:
  required_fields:
    id: true
# reload
"#,
    )
    .unwrap();

    let changes = vec![FileEvent {
        uri: Url::from_file_path(&config_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }];

    // Drain any buffered scan_done messages before triggering the change
    while scan_done_rx.try_recv().is_ok() {}

    service
        .call(
            tower_lsp::jsonrpc::Request::build("workspace/didChangeWatchedFiles")
                .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_millis(2000), scan_done_rx.recv())
        .await
        .expect("Reload workspace scan did not complete in time")
        .expect("scan_done_rx closed before reload scan completed");

    let after_reload: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;
    let after_items = match after_reload {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.items
        }
        _ => panic!("Expected full diagnostic report after reload"),
    };
    assert!(
        after_items.iter().all(|diag| {
            diag.code != Some(NumberOrString::String("required_field_missing".to_string()))
        }),
        "Pull diagnostics should be refreshed after reload so cross-file fragment fields remain visible: {after_items:#?}"
    );
}
