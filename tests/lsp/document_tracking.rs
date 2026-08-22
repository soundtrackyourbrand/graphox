use crate::support::{self, TestWorkspace, create_initialized_lsp_service_with_socket};
use graphox::Config;
use std::fs;
use std::time::Duration;
use tokio_stream::StreamExt;
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(5000)]
async fn test_document_close_reverts_to_disk_state_on_reload() {
    let ws = TestWorkspace::new();
    let root = ws.root();

    // 1. Create initial project files (valid content on disk)
    let schema_text = "type Query { user: User } type User { id: ID! name: String }";
    ws.write_file("schema.graphql", schema_text);

    let config_text = r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
"#;
    ws.write_file("graphox.yaml", config_text);

    // Valid query on disk
    let query_text_disk = "query GetUser { user { id } }";
    let query_path = ws.write_file("query.graphql", query_text_disk);
    let query_path = fs::canonicalize(&query_path).unwrap();
    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();

    // 2. Initialize LSP
    let config = Config::load_from_dir(root).unwrap().unwrap();
    let (mut service, mut messages) = create_initialized_lsp_service_with_socket(config).await;

    // 3. Open document with INVALID content (referencing missing field)
    let query_text_invalid = "query GetUser { user { missingField } }";
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text_invalid.to_string(),
        },
    };
    let req = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(open_params).unwrap())
        .finish();
    service.call(req).await.unwrap();

    // 4. Verify diagnostic appears for invalid in-memory content
    let mut found_error = false;
    let timeout = Duration::from_secs(2);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(100), messages.next()).await {
            Ok(Some(msg)) => {
                let method = msg["method"].as_str().unwrap_or("");
                if method == "textDocument/publishDiagnostics" {
                    let params: PublishDiagnosticsParams =
                        serde_json::from_value(msg["params"].clone()).unwrap();
                    if params.uri == query_uri
                        && !params.diagnostics.is_empty()
                        && params.diagnostics.iter().any(|d| {
                            d.message.contains("missingField") && d.message.contains("not found")
                        })
                    {
                        found_error = true;
                        break;
                    }
                }
            }
            _ => continue,
        }
    }
    assert!(
        found_error,
        "Expected error diagnostic for 'missingField' in unsaved document"
    );

    // 5. Close document
    let close_params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
    };
    let req = Request::build("textDocument/didClose")
        .params(serde_json::to_value(close_params).unwrap())
        .finish();
    service.call(req).await.unwrap();

    // 6. Trigger config reload (modify config file)
    // This will cause the backend to reload state. If didClose correctly updated open_documents,
    // it should now use the valid disk content instead of the invalid memory content.
    let new_config_text = format!("{}\n# reload trigger", config_text);
    fs::write(root.join("graphox.yaml"), new_config_text).unwrap();

    let changes = vec![FileEvent {
        uri: graphox::utils::path_to_uri(root.join("graphox.yaml")).unwrap(),
        typ: FileChangeType::CHANGED,
    }];
    let reload_req = Request::build("workspace/didChangeWatchedFiles")
        .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
        .finish();
    service.call(reload_req).await.unwrap();

    // 7. Verify diagnostics are CLEARED (or updated to reflect valid disk state)
    // Since the disk state is valid, we expect zero diagnostics.
    let report = support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    match report {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            assert!(
                full.full_document_diagnostic_report.items.is_empty(),
                "Diagnostics did not revert to valid disk state after closing document and reloading config. Got: {:?}",
                full.full_document_diagnostic_report.items
            );
        }
        _ => panic!("Expected full diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(30000)]
async fn test_config_reload_preserves_open_document_state() {
    let ws = TestWorkspace::new();
    let root = ws.root();

    // 1. Setup workspace
    let schema_text = "type Query { user: User } type User { id: ID! }";
    ws.write_file("schema.graphql", schema_text);

    let config_text = r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
"#;
    ws.write_file("graphox.yaml", config_text);

    let query_text_disk = "query GetUser { user { id } }";
    let query_path = ws.write_file("query.graphql", query_text_disk);
    let query_path = fs::canonicalize(&query_path).unwrap();
    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();

    // 2. Initialize LSP
    let config = Config::load_from_dir(root).unwrap().unwrap();
    let (mut service, _messages) = create_initialized_lsp_service_with_socket(config).await;

    // 3. Open document with INVALID content
    let query_text_invalid = "query GetUser { user { INVALID_FIELD } }";
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text_invalid.to_string(),
        },
    };
    let req = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(open_params).unwrap())
        .finish();
    service.call(req).await.unwrap();

    // 4. Verify diagnostic appears
    let report = support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    match report {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            let found_error = full
                .full_document_diagnostic_report
                .items
                .iter()
                .any(|d| d.message.contains("INVALID_FIELD"));
            assert!(
                found_error,
                "Expected error for INVALID_FIELD. Got: {:?}",
                full.full_document_diagnostic_report.items
            );
        }
        _ => panic!("Expected full diagnostic report"),
    }

    // 5. Reload config WITHOUT closing document
    let new_config_text = format!("{}\n# reload trigger 2", config_text);
    fs::write(root.join("graphox.yaml"), new_config_text).unwrap();

    let changes = vec![FileEvent {
        uri: graphox::utils::path_to_uri(root.join("graphox.yaml")).unwrap(),
        typ: FileChangeType::CHANGED,
    }];
    let reload_req = Request::build("workspace/didChangeWatchedFiles")
        .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
        .finish();
    service.call(reload_req).await.unwrap();

    // 6. Verify diagnostic STILL exists (meaning memory state was preserved)
    let report = support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    match report {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            let error_persists = full
                .full_document_diagnostic_report
                .items
                .iter()
                .any(|d| d.message.contains("INVALID_FIELD"));
            assert!(
                error_persists,
                "Error should persist after config reload if document is still open. Got: {:?}",
                full.full_document_diagnostic_report.items
            );
        }
        _ => panic!("Expected full diagnostic report"),
    }
}
