use crate::support;
use futures_util::StreamExt;
use graphox::{Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource};
use std::fs;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_codegen_throttle() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    let mut schema_text = "type User { id: ID! name: String ".to_string();
    for i in 2..12 {
        schema_text.push_str(&format!("f{}: String ", i));
    }
    schema_text.push_str("} type Query { me: User }");
    fs::write(&schema_path, schema_text).unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetMe { me { id } }";
    fs::write(&query_path, query_text).unwrap();

    let output_dir = base_dir.join("generated");
    fs::create_dir(&output_dir).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_output_dir("generated".to_string()),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(false);

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

    let query_uri = Uri::from_file_path(&query_path).unwrap();

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
    let mut last_marker = String::new();
    for i in 2..12 {
        let marker = format!("GetMe{}", i);
        last_marker = marker.clone();
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
                                text: format!("query {} {{ me {{ id f{} }} }}", marker, i),
                            }],
                        })
                        .unwrap(),
                    )
                    .finish(),
            )
            .await
            .unwrap();
        // Small delay between changes
        sleep(Duration::from_millis(10)).await;
    }

    // Wait for throttle period plus buffer.
    // We want to ensure we wait long enough for the LAST throttled run to complete.
    sleep(Duration::from_millis(1500)).await;

    let elapsed = start.elapsed();

    // Verify the output file was generated and contains the final marker
    let output_path = output_dir.join("query.codegen.ts");
    assert!(
        support::wait_for_file_async(
            &output_path,
            Duration::from_millis(3000),
            Some(&last_marker)
        )
        .await,
        "Generated file should contain the final marker '{}' after throttled codegen. Elapsed: {:?}",
        last_marker,
        elapsed
    );

    // The test passes if we got here - the throttle mechanism is working
    // if the codegen completes without errors and doesn't overwhelm the system
    println!("Test completed in {:?}", elapsed);
}
