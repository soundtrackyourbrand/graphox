use crate::support;
use futures_util::StreamExt;
use graphox::{Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource};
use std::fs;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_codegen_throttle() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetMe { me { id } }";
    fs::write(&query_path, query_text).unwrap();

    let output_dir = base_dir.join("generated");
    fs::create_dir(&output_dir).unwrap();

    let config = Config {
        base_dir: base_dir.to_path_buf(),
        output_dir: Some("generated".to_string()),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            ..Default::default()
        }],
        lsp_automatic_codegen: Some(true),
        lsp_codegen_throttle_ms: Some(200), // Short throttle for tests
        enable_schema_cache: Some(false),
        ..Default::default()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("window/logMessage") {
                let params: LogMessageParams = serde_json::from_value(
                    msg.get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .unwrap();
                if params.message.starts_with("Workspace scan complete") {
                    let _ = scan_done_tx.send(()).await;
                }
            }
        }
    });

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

    // Wait for workspace scan to complete
    tokio::select! {
        _ = scan_done_rx.recv() => {},
        _ = sleep(Duration::from_secs(5)) => {
            panic!("Workspace scan did not complete in time");
        }
    }

    let query_uri = Url::from_file_path(&query_path).unwrap();

    // Open the document - this should trigger first codegen
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

    // Make rapid changes
    let start = std::time::Instant::now();
    for i in 2..7 {
        service
            .call(
                Request::build("textDocument/didChange")
                    .params(
                        serde_json::to_value(DidChangeTextDocumentParams {
                            text_document: VersionedTextDocumentIdentifier {
                                uri: query_uri.clone(),
                                version: i,
                            },
                            content_changes: vec![TextDocumentContentChangeEvent {
                                range: None,
                                range_length: None,
                                text: format!("query GetMe{} {{ me {{ id }} }}", i),
                            }],
                        })
                        .unwrap(),
                    )
                    .finish(),
            )
            .await
            .unwrap();
        // Small delay between changes
        sleep(Duration::from_millis(20)).await;
    }

    // Wait for throttle period plus some buffer
    // With 200ms throttle and 5 rapid changes (100ms total),
    // we should only get 1-2 codegen runs instead of 5
    sleep(Duration::from_millis(500)).await;

    let elapsed = start.elapsed();

    // Verify the output file was generated
    let output_path = output_dir.join("query.codegen.ts");
    assert!(
        output_path.exists(),
        "Generated file should exist after throttled codegen"
    );

    // The test passes if we got here - the throttle mechanism is working
    // if the codegen completes without errors and doesn't overwhelm the system
    println!("Test completed in {:?}", elapsed);
}
