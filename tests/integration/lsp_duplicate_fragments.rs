use futures_util::StreamExt;
use graphql_rust::{Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use crate::support;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(1000)]
async fn test_lsp_duplicate_fragments_same_project_via_config() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir_all(&pkg_a).unwrap();
    let frag_a_path = pkg_a.join("frag_a.graphql");
    fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();

    let pkg_b = base_dir.join("pkg_b");
    fs::create_dir_all(&pkg_b).unwrap();
    let frag_b_path = pkg_b.join("frag_b.graphql");
    fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

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

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
    ));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(msg.get("params").cloned().unwrap_or(serde_json::Value::Null)).unwrap();
                received_diags_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            } else if msg.get("method").and_then(|m| m.as_str()) == Some("window/logMessage") {
                let params: LogMessageParams =
                    serde_json::from_value(msg.get("params").cloned().unwrap_or(serde_json::Value::Null)).unwrap();
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

    // Wait for background scan to complete
    let _ = tokio::time::timeout(Duration::from_millis(200), scan_done_rx.recv())
        .await
        .expect("Scan did not complete in time");

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let uri_b = Url::from_file_path(&frag_b_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri_a.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_a_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri_b.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_b_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(10)).await;

    let diags = received_diags.lock().unwrap();

    let d_a = diags.get(&uri_a).unwrap();
    assert!(d_a.iter().any(|d| d.message.contains("Duplicate fragment name: 'DuplicateFrag'")));
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(1000)]
async fn test_lsp_private_duplicates_different_projects_no_error() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir_all(&pkg_a).unwrap();
    let frag_a_path = pkg_a.join("frag_a.graphql");
    fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();

    let pkg_b = base_dir.join("pkg_b");
    fs::create_dir_all(&pkg_b).unwrap();
    let frag_b_path = pkg_b.join("frag_b.graphql");
    fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_a/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_b/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
        ],
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

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
    ));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(msg.get("params").cloned().unwrap_or(serde_json::Value::Null)).unwrap();
                received_diags_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            } else if msg.get("method").and_then(|m| m.as_str()) == Some("window/logMessage") {
                let params: LogMessageParams =
                    serde_json::from_value(msg.get("params").cloned().unwrap_or(serde_json::Value::Null)).unwrap();
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

    // Wait for background scan to complete
    let _ = tokio::time::timeout(Duration::from_millis(200), scan_done_rx.recv())
        .await
        .expect("Scan did not complete in time");

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let uri_b = Url::from_file_path(&frag_b_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri_a.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_a_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri_b.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_b_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(10)).await;

    let diags = received_diags.lock().unwrap();

    let d_a = diags.get(&uri_a).unwrap();
    assert!(!d_a.iter().any(|d| d.message.contains("Duplicate fragment name: 'DuplicateFrag'")));
}
