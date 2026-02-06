use futures_util::StreamExt;
use graphql_rust::{
    Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_workspace_scan_concurrency() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Create a schema
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    // Create many files to make the scan take at least some time
    // Even if it's fast, we want to test concurrent access
    for i in 0..100 {
        let path = base_dir.join(format!("file_{}.graphql", i));
        fs::write(&path, format!("query Query{} {{ me {{ id }} }}", i)).unwrap();
    }

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
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
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        rules: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let progress_updates = Arc::new(Mutex::new(Vec::new()));
    let progress_updates_clone = progress_updates.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "window/logMessage" {
                let params: LogMessageParams =
                    serde_json::from_value(msg.params().unwrap().clone()).unwrap();
                if params.message.starts_with("Workspace scan complete") {
                    let _ = scan_done_tx.send(()).await;
                }
            } else if msg.method() == "$/progress" {
                progress_updates_clone
                    .lock()
                    .unwrap()
                    .push(msg.params().unwrap().clone());
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
    let new_file_path = base_dir.join("new_file.graphql");
    let new_file_text = "query NewQuery { me { name } }";
    fs::write(&new_file_path, new_file_text).unwrap();
    let new_file_uri = Url::from_file_path(&new_file_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: new_file_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: new_file_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 3. Request hover immediately
    let hover_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: new_file_uri.clone(),
            },
            position: Position::new(0, 22), // on 'name'
        },
        work_done_progress_params: Default::default(),
    };

    let hover_request = Request::build("textDocument/hover")
        .id(2)
        .params(serde_json::to_value(&hover_params).unwrap())
        .finish();

    let hover_response = service.call(hover_request).await.unwrap().unwrap();
    let hover_result: Option<Hover> =
        serde_json::from_value(hover_response.result().unwrap().clone()).unwrap();

    assert!(
        hover_result.is_some(),
        "Hover should return a result even during workspace scan"
    );

    // 4. Wait for scan to complete
    let _ = tokio::time::timeout(Duration::from_secs(5), scan_done_rx.recv())
        .await
        .expect("Workspace scan did not complete");
}
