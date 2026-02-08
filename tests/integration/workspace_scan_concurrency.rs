use crate::support::{self, lsp_did_open, lsp_request_hover, pos};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, sleep};
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn test_workspace_scan_concurrency() {
    // Build a scenario with a schema and many query files so the workspace
    // scan has some work to do.
    let mut scenario = crate::support::lsp::LspTestScenario::new().with_file(
        "schema.graphql",
        "type User { id: ID! name: String } type Query { me: User }",
    );

    for i in 0..100 {
        scenario = scenario.with_file(
            &format!("file_{}.graphql", i),
            &format!("query Query{} {{ me {{ id }} }}", i),
        );
    }

    let base_dir = scenario.write_files().unwrap();
    let config = scenario.build_config(&base_dir);
    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let progress_updates = Arc::new(Mutex::new(Vec::new()));
    let progress_updates_clone = progress_updates.clone();

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
            } else if msg.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                progress_updates_clone.lock().unwrap().push(
                    msg.get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                );
            }
        }
    });

    // 1. Initialize
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(1)
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

    // The scan is now running in the background.

    // 2. Immediately open a new file
    let new_file_text = "query NewQuery { me { name } }";
    let new_file_uri =
        crate::support::write_project_file_at(&base_dir, "new_file.graphql", new_file_text);

    lsp_did_open(
        &mut service,
        new_file_uri.clone(),
        "graphql",
        1,
        new_file_text,
    )
    .await;

    // 3. Request hover immediately
    sleep(Duration::from_millis(2)).await;
    let hover_result = lsp_request_hover(&mut service, new_file_uri.clone(), pos(0, 22)).await;

    assert!(
        hover_result.is_some(),
        "Hover should return a result even during workspace scan"
    );

    // 4. Wait for scan to complete
    let _ = tokio::time::timeout(Duration::from_secs(5), scan_done_rx.recv())
        .await
        .expect("Workspace scan did not complete");
}
