use crate::support::{
    TestWorkspace, assert_diagnostic_with_message, create_initialized_lsp_service_with_socket,
};
use graphox::Config;
use std::fs;
use std::time::Duration;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(3000)]
async fn test_config_reload_preserves_unsaved_changes() {
    let ws = TestWorkspace::new();
    let root = ws.root();

    // 1. Create initial project files (valid content on disk)
    let schema_text = "type Query { user: User } type User { id: ID! name: String }";
    ws.write_file("schema.graphql", schema_text);

    let config_text = r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
rules:
  no_duplicate_fields: true
"#;
    ws.write_file("graphox.yaml", config_text);

    // Valid query on disk
    let query_text_disk = "query GetUser { user { id } }";
    let query_path = ws.write_file("query.graphql", query_text_disk);
    let query_path = fs::canonicalize(&query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    // 2. Initialize LSP
    let config = Config::load_from_dir(root).unwrap().unwrap();
    let (mut service, mut messages) = create_initialized_lsp_service_with_socket(config).await;

    // 3. Open document with UNSAVED changes that trigger a diagnostic (duplicate field)
    let query_text_unsaved = "query GetUser { user { id id } }";
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text_unsaved.to_string(),
        },
    };
    let req = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(open_params).unwrap())
        .finish();
    service.call(req).await.unwrap();

    // 4. Verify diagnostic appears for unsaved content
    let mut found_diag = false;
    let timeout = Duration::from_secs(1);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        use tokio_stream::StreamExt;
        match tokio::time::timeout(Duration::from_millis(100), messages.next()).await {
            Ok(Some(msg)) => {
                let method = msg["method"].as_str().unwrap_or("");
                if method == "textDocument/publishDiagnostics" {
                    let params: PublishDiagnosticsParams =
                        serde_json::from_value(msg["params"].clone()).unwrap();
                    if params.uri == query_uri && !params.diagnostics.is_empty() {
                        assert_diagnostic_with_message(&params.diagnostics, "Duplicate field");
                        found_diag = true;
                        break;
                    }
                }
            }
            _ => continue,
        }
    }
    assert!(
        found_diag,
        "Expected diagnostic for duplicate field in unsaved document"
    );

    // 5. Trigger config reload (modify config file)
    let new_config_text = format!("{}\n# reload trigger", config_text);
    fs::write(root.join("graphox.yaml"), new_config_text).unwrap();

    let changes = vec![FileEvent {
        uri: Url::from_file_path(root.join("graphox.yaml")).unwrap(),
        typ: FileChangeType::CHANGED,
    }];
    let reload_req = Request::build("workspace/didChangeWatchedFiles")
        .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
        .finish();
    service.call(reload_req).await.unwrap();

    // 6. Verify diagnostic PERSISTS after reload
    let mut found_diag_after_reload = false;
    let check_duration = Duration::from_secs(4);
    let check_start = std::time::Instant::now();

    while check_start.elapsed() < check_duration {
        use tokio_stream::StreamExt;
        match tokio::time::timeout(Duration::from_millis(100), messages.next()).await {
            Ok(Some(msg)) => {
                let method = msg["method"].as_str().unwrap_or("");
                if method == "textDocument/publishDiagnostics" {
                    let params: PublishDiagnosticsParams =
                        serde_json::from_value(msg["params"].clone()).unwrap();
                    if params.uri == query_uri {
                        if params.diagnostics.is_empty() {
                            panic!(
                                "Diagnostics were cleared after config reload (unsaved changes lost!)"
                            );
                        } else {
                            assert_diagnostic_with_message(&params.diagnostics, "Duplicate field");
                            found_diag_after_reload = true;
                            break; // Success!
                        }
                    }
                }
            }
            _ => continue,
        }
    }

    assert!(
        found_diag_after_reload,
        "Did not receive diagnostics after config reload"
    );
}
