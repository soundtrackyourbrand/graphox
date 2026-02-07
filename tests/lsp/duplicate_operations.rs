use futures_util::StreamExt;
use std::sync::atomic::Ordering;
use graphql_rust::config::RulesConfig;
use std::fs;
use tempfile::tempdir;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
#[ntest::timeout(10000)]
async fn test_duplicate_operation_names_cross_file() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema = "type User { id: ID! name: String! } type Query { user(id: ID!): User }";
    let (tmpdir, mut config) = crate::support::make_temp_project_with_schema(schema, "**/*.graphql");
    // create the query files before initialization so the workspace scan discovers them
    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    let query2_text = "query GetUser { user(id: \"2\") { id } }";
    let query1_path = crate::support::write_project_file(&tmpdir, "query1.graphql", query1_text);
    let query2_path = crate::support::write_project_file(&tmpdir, "query2.graphql", query2_text);

    // Update config base_dir to the temp dir returned by helper
    config.base_dir = tmpdir.path().to_path_buf();

    // Enable the unique operation name rule for this test so duplicate
    // operation diagnostics are produced during validation.
    config.rules = Some(RulesConfig {
        required_fields: None,
        unique_operation_name: Some(true),
        no_duplicate_fields: None,
    });

    // Create service and capture server->client messages so we can assert push diagnostics
    let (mut service, mut messages) = crate::support::create_lsp_service_with_socket(config);
    let received_push = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<Url, Vec<tower_lsp::lsp_types::Diagnostic>>::new()));
    let received_push_clone = received_push.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            let msg: serde_json::Value = msg; // concrete type for inference
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params: PublishDiagnosticsParams = serde_json::from_value(msg.get("params").cloned().unwrap_or(serde_json::Value::Null)).unwrap();
                received_push_clone.lock().unwrap().insert(params.uri, params.diagnostics);
            }
        }
    });

    // Initialize the LSP session
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
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

    // Wait for workspace scan to complete (backend.workspace_loaded)
    let backend = service.inner();
    let start = std::time::Instant::now();
    while !backend.workspace_loaded.load(Ordering::SeqCst) {
        if start.elapsed().as_secs() > 5 {
            panic!("workspace scan did not complete in time");
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let query1_uri = query1_path;
    let query2_uri = query2_path;

    // Open the documents (they're already scanned by workspace scan, but we need to open them for diagnostics)
    crate::support::lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;
    crate::support::lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Request diagnostics for first file
    let params1 = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: query1_uri.clone(),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request1 = Request::build("textDocument/diagnostic")
        .id(2)
        .params(serde_json::to_value(&params1).unwrap())
        .finish();
    // Poll for diagnostics until we find the duplicate diagnostic or timeout
    let mut found_dup = false;
    let mut last_diags_json: Option<String> = None;
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 2000 {
        let response1 = service.call(request1.clone()).await.unwrap().unwrap();
        let result1: DocumentDiagnosticReportResult =
            serde_json::from_value(response1.result().unwrap().clone()).unwrap();

        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) = result1 {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            last_diags_json = Some(serde_json::to_string_pretty(&diagnostics).unwrap_or_default());
            if diagnostics.iter().any(|d| d.message.contains("Duplicate operation name 'GetUser'")) {
                found_dup = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(found_dup, "Should find duplicate operation diagnostic in query1.graphql; last diagnostics: {:?}", last_diags_json);

    // Request diagnostics for second file
    let params2 = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: query2_uri.clone(),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request2 = Request::build("textDocument/diagnostic")
        .id(3)
        .params(serde_json::to_value(&params2).unwrap())
        .finish();
    // Poll for diagnostics for query2 as well
    let mut found_dup2 = false;
    let mut last_diags_json2: Option<String> = None;
    let start2 = std::time::Instant::now();
    while start2.elapsed().as_millis() < 2000 {
        let response2 = service.call(request2.clone()).await.unwrap().unwrap();
        let result2: DocumentDiagnosticReportResult =
            serde_json::from_value(response2.result().unwrap().clone()).unwrap();

        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) = result2 {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            last_diags_json2 = Some(serde_json::to_string_pretty(&diagnostics).unwrap_or_default());
            if diagnostics.iter().any(|d| d.message.contains("Duplicate operation name 'GetUser'")) {
                found_dup2 = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(found_dup2, "Should find duplicate operation diagnostic in query2.graphql; last diagnostics: {:?}", last_diags_json2);
}

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_unique_operation_names_no_duplicates() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
    )
    .unwrap();

    // Use test helpers to create a workspace with schema and initialize LSP
    let schema = "type User { id: ID! name: String! } type Query { user(id: ID!): User }";
    let (tmpdir, config) = crate::support::make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    // Create first file with GetUser operation
    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    let query1_uri = crate::support::write_project_file(&tmpdir, "query1.graphql", query1_text);
    crate::support::lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;

    // Create second file with different operation name
    let query2_text = "query GetUserById { user(id: \"2\") { id } }";
    let query2_uri = crate::support::write_project_file(&tmpdir, "query2.graphql", query2_text);
    crate::support::lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Poll for diagnostics to ensure they are processed
    let mut diags_empty = false;
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 1000 {
        // Request diagnostics for first file using helper
        let result1 = crate::support::lsp_request_diagnostics(&mut service, query1_uri.clone()).await;

        // Check that we got NO duplicate operation diagnostics
        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) = result1 {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            let duplicate_diags: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.message.contains("Duplicate operation"))
                .collect();

            if duplicate_diags.is_empty() {
                diags_empty = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(diags_empty, "Should not find duplicate operation diagnostics when names are unique");
}
