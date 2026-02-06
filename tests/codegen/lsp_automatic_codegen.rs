use futures_util::StreamExt;
use graphql_rust::{
    Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use std::fs;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen() {
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

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        lsp_automatic_codegen: Some(true),
        lsp_codegen_throttle_ms: Some(50), // Short throttle for tests
        timeouts: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "window/logMessage" {
                let params: LogMessageParams =
                    serde_json::from_value(msg.params().unwrap().clone()).unwrap();
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

    let query_uri = Url::from_file_path(&query_path).unwrap();
    let gen_path = base_dir.join("query.codegen.ts");

    // 1. Initial codegen (triggered by didOpen if we wanted, but let's test didChange)
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

    // Wait for codegen
    wait_for_file(&gen_path, Duration::from_millis(200)).await;
    let content = fs::read_to_string(&gen_path).unwrap();
    assert!(content.contains("GetMe"));
    // Use a more specific check to avoid matching schema types or comments if any
    assert!(
        !content.contains("name: string"),
        "Generated content should not contain 'name' field: {}",
        content
    );

    // 2. Test didChange triggers codegen
    let query_text_new = "query GetMe { me { id name } }";
    // In this implementation, codegen currently reads from disk, so we must save the file
    // In a real editor, this happens on save or via auto-save.
    fs::write(&query_path, query_text_new).unwrap();
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: query_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: query_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for updated codegen
    let mut updated = false;
    for _ in 0..40 {
        if let Ok(c) = fs::read_to_string(&gen_path)
            && c.contains("name: string | null")
        {
            updated = true;
            break;
        }
        sleep(Duration::from_millis(1)).await;
    }
    assert!(updated, "Codegen was not updated after didChange");

    // 3. Test didChangeWatchedFiles triggers codegen
    fs::remove_file(&gen_path).unwrap();
    let query_text_watched = "query GetMe { me { name } }";
    fs::write(&query_path, query_text_watched).unwrap();

    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(
                    serde_json::to_value(DidChangeWatchedFilesParams {
                        changes: vec![FileEvent {
                            uri: query_uri.clone(),
                            typ: FileChangeType::CHANGED,
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for codegen
    wait_for_file(&gen_path, Duration::from_millis(200)).await;
    let content = fs::read_to_string(&gen_path).unwrap();
    assert!(content.contains("name: string | null"));
    assert!(!content.contains("id: string"));
}

async fn wait_for_file(path: &std::path::Path, timeout: Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("Timeout waiting for file {}", path.display());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_disabled() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User users: [User!]! }",
    )
    .unwrap();

    // Create two query files - one for enabled project, one for disabled
    let enabled_query_path = base_dir.join("enabled.graphql");
    let enabled_query_text = "query GetMe { me { id } }";
    fs::write(&enabled_query_path, enabled_query_text).unwrap();

    let disabled_query_path = base_dir.join("disabled.graphql");
    let disabled_query_text = "query GetUsers { users { id name } }";
    fs::write(&disabled_query_path, disabled_query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("enabled.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(true), // Explicitly enabled
            },
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("disabled.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false), // Disabled
            },
        ],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        lsp_automatic_codegen: Some(true),
        lsp_codegen_throttle_ms: Some(50), // Short throttle for tests
        timeouts: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "window/logMessage" {
                let params: LogMessageParams =
                    serde_json::from_value(msg.params().unwrap().clone()).unwrap();
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

    let enabled_uri = Url::from_file_path(&enabled_query_path).unwrap();
    let disabled_uri = Url::from_file_path(&disabled_query_path).unwrap();
    let enabled_gen_path = base_dir.join("enabled.codegen.ts");
    let disabled_gen_path = base_dir.join("disabled.codegen.ts");

    // Open enabled query file - should trigger codegen
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: enabled_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: enabled_query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for enabled codegen
    wait_for_file(&enabled_gen_path, Duration::from_millis(1000)).await;
    let enabled_content = fs::read_to_string(&enabled_gen_path).unwrap();
    assert!(enabled_content.contains("GetMeQuery"));

    // Open disabled query file - should NOT trigger codegen
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: disabled_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: disabled_query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait a bit to ensure no codegen happens
    sleep(Duration::from_millis(10)).await;

    // Verify disabled project did NOT generate files
    assert!(
        !disabled_gen_path.exists(),
        "Should not generate files for disabled project, but found: {}",
        disabled_gen_path.display()
    );

    // Test didChange on disabled project - should still not generate
    let disabled_query_text_new = "query GetUsers { users { id name } }";
    fs::write(&disabled_query_path, disabled_query_text_new).unwrap();
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: disabled_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: disabled_query_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait again to ensure no codegen happens
    sleep(Duration::from_millis(500)).await;
    assert!(
        !disabled_gen_path.exists(),
        "Should still not generate files after didChange for disabled project"
    );

    // Verify enabled project still works
    let enabled_query_text_new = "query GetMe { me { id name } }";
    fs::write(&enabled_query_path, enabled_query_text_new).unwrap();
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: enabled_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: enabled_query_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for updated codegen on enabled project
    let mut updated = false;
    for _ in 0..20 {
        if let Ok(c) = fs::read_to_string(&enabled_gen_path)
            && c.contains("name: string | null")
        {
            updated = true;
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(
        updated,
        "Enabled project codegen was not updated after didChange"
    );
}
