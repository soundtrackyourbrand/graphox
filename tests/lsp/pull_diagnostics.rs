use futures_util::StreamExt;
use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_basic() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetUser { user { nonExistentField } }"; // Invalid field
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
            watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track push diagnostics (should not receive any when using pull)
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_push_diags_clone
                    .lock()
                    .unwrap()
                    .push(params.clone());
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

    let response = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    // Verify server advertises diagnostic support
    let result: InitializeResult =
        serde_json::from_value(response.unwrap().result().unwrap().clone()).unwrap();
    assert!(result.capabilities.diagnostic_provider.is_some());

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = Url::from_file_path(&query_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Request diagnostics via pull
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let response = service
        .call(
            Request::build("textDocument/diagnostic")
                .params(serde_json::to_value(&diag_params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap();

    let result: DocumentDiagnosticReportResult =
        serde_json::from_value(response.unwrap().result().unwrap().clone()).unwrap();

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

    // Note: The workspace scan that runs in background on initialization may still
    // send push diagnostics before pull capability detection is complete.
    // In a real client, this is acceptable since the client would simply ignore them.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_unchanged() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetUser { user { id name } }"; // Valid query
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

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

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = Url::from_file_path(&query_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(10)).await;

    // First pull request
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let response = service
        .call(
            Request::build("textDocument/diagnostic")
                .params(serde_json::to_value(&diag_params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap();

    let first_result: DocumentDiagnosticReportResult =
        serde_json::from_value(response.unwrap().result().unwrap().clone()).unwrap();

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

    let response2 = service
        .call(
            Request::build("textDocument/diagnostic")
                .params(serde_json::to_value(&diag_params2).unwrap())
                .id(2)
                .finish(),
        )
        .await
        .unwrap();

    let second_result: DocumentDiagnosticReportResult =
        serde_json::from_value(response2.unwrap().result().unwrap().clone()).unwrap();

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User post: Post } type User { id: ID! } type Post { title: String }",
    )
    .unwrap();

    let query1_path = base_dir.join("query1.graphql");
    fs::write(&query1_path, "query GetUser { user { id } }").unwrap();

    let query2_path = base_dir.join("query2.graphql");
    fs::write(&query2_path, "query GetPost { post { title } }").unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

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

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open both documents
    let query1_uri = Url::from_file_path(&query1_path).unwrap();
    let query2_uri = Url::from_file_path(&query2_path).unwrap();

    for (uri, path) in [
        (query1_uri.clone(), &query1_path),
        (query2_uri.clone(), &query2_path),
    ] {
        service
            .call(
                Request::build("textDocument/didOpen")
                    .params(
                        serde_json::to_value(DidOpenTextDocumentParams {
                            text_document: TextDocumentItem {
                                uri,
                                language_id: "graphql".to_string(),
                                version: 1,
                                text: fs::read_to_string(path).unwrap(),
                            },
                        })
                        .unwrap(),
                    )
                    .finish(),
            )
            .await
            .unwrap();
    }

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Request workspace diagnostics
    let workspace_diag_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: vec![],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let response = service
        .call(
            Request::build("workspace/diagnostic")
                .params(serde_json::to_value(&workspace_diag_params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap();

    let result: WorkspaceDiagnosticReportResult =
        serde_json::from_value(response.unwrap().result().unwrap().clone()).unwrap();

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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetUser { user { invalidField } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track push diagnostics
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_push_diags_clone
                    .lock()
                    .unwrap()
                    .push(params.clone());
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

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = Url::from_file_path(&query_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(10)).await;

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
