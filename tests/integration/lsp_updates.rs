use futures_util::StreamExt;
use graphql_rust::{
    Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(1000)]
async fn test_lsp_fragment_collisions() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir_all(&pkg_a).unwrap();
    let frag_a_path = pkg_a.join("frag.graphql");
    fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();

    let frag_b_path = pkg_a.join("other.graphql");
    fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let pkg_b = base_dir.join("pkg_b");
    fs::create_dir_all(&pkg_b).unwrap();
    let frag_c_path = pkg_b.join("pub.graphql");
    fs::write(&frag_c_path, "fragment PublicFrag on User @public { id }").unwrap();

    let frag_d_path = pkg_a.join("shadow.graphql");
    fs::write(&frag_d_path, "fragment PublicFrag on User { name }").unwrap();

    let frag_e_path = pkg_b.join("pub_collision.graphql");
    fs::write(
        &frag_e_path,
        "fragment PublicCollision on User @public { id }",
    )
    .unwrap();

    let frag_f_path = pkg_a.join("pub_collision_2.graphql");
    fs::write(
        &frag_f_path,
        "fragment PublicCollision on User @public { name }",
    )
    .unwrap();

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
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
    ));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(msg.params().unwrap().clone()).unwrap();
                received_diags_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            } else if msg.method() == "window/logMessage" {
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

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let uri_b = Url::from_file_path(&frag_b_path).unwrap();
    let uri_c = Url::from_file_path(&frag_c_path).unwrap();
    let uri_d = Url::from_file_path(&frag_d_path).unwrap();
    let uri_e = Url::from_file_path(&frag_e_path).unwrap();
    let uri_f = Url::from_file_path(&frag_f_path).unwrap();

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
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: uri_c.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_c_path).unwrap(),
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
                            uri: uri_d.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_d_path).unwrap(),
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
                            uri: uri_e.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_e_path).unwrap(),
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
                            uri: uri_f.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&frag_f_path).unwrap(),
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

    // Check private collision in pkg_a
    let d_a = diags.get(&uri_a).unwrap();
    assert!(d_a.iter().any(|d| {
        d.message
            .contains("Duplicate fragment name: 'DuplicateFrag'")
    }));

    // Check shadowing in shadow.graphql
    let d_d = diags.get(&uri_d).unwrap();
    assert!(
        d_d.iter()
            .any(|d| d.message.contains("shadows a public fragment"))
    );

    // Check public collision
    let d_e = diags.get(&uri_e).unwrap();
    assert!(d_e.iter().any(|d| {
        d.message
            .contains("Duplicate public fragment name: 'PublicCollision'")
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(1000)]
async fn test_lsp_diagnostics_on_schema_change() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();
    let schema_path = schema_path.canonicalize().unwrap();
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = query_path.canonicalize().unwrap();
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
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let client_capture = Arc::new(Mutex::new(None));
    let client_capture_clone = client_capture.clone();
    let (mut service, mut messages) = LspService::new(|client| {
        let mut cap = client_capture_clone.lock().unwrap();
        *cap = Some(client.clone());
        Backend::new(client, config)
    });
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            } else if msg.method() == "window/logMessage" {
                let params: LogMessageParams =
                    serde_json::from_value(msg.params().unwrap().clone()).unwrap();
                if params.message.starts_with("Workspace scan complete") {
                    let _ = scan_done_tx.send(()).await;
                }
            }
        }
    });
    // We need to trigger the closure by making any call to the service
    let init_params = InitializeParams {
        ..Default::default()
    };
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await;

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

    // Open document
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();
    // Wait for initial diagnostics
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        assert!(
            !diags.is_empty(),
            "Should have received initial diagnostics"
        );
        assert!(
            diags.last().unwrap()["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    // 4. Change schema on disk: rename 'name' to 'fullName'
    fs::write(
        &schema_path,
        "type User { id: ID! fullName: String } type Query { me: User }",
    )
    .unwrap();
    // 5. Notify LSP about the change
    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Url::from_file_path(&schema_path).unwrap(),
            typ: FileChangeType::CHANGED,
        }],
    };
    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();
    // 6. Wait for diagnostics after schema change
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let last = diags.last().unwrap();
        let d_list = last["diagnostics"].as_array().unwrap();
        assert!(
            !d_list.is_empty(),
            "Should have diagnostics after schema change"
        );
        assert!(d_list[0]["message"].as_str().unwrap().contains("name"));
    }
    // 7. Fix the query
    let query_text_fixed = "query { me { id fullName } }";
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: query_uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: query_text_fixed.to_string(),
        }],
    };
    service
        .call(
            Request::build("textDocument/didChange")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();
    // 8. Verify diagnostics cleared
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let last = diags.last().unwrap();
        assert!(last["diagnostics"].as_array().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(1000)]
async fn test_lsp_fragment_rename_same_project() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();
    let frag_path = base_dir.join("frag.graphql");
    let frag_text = "fragment UserFrag on User { id }";
    fs::write(&frag_path, frag_text).unwrap();
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { ...UserFrag } }";
    fs::write(&query_path, query_text).unwrap();
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
        timeouts: None,
    };
    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            } else if msg.method() == "window/logMessage" {
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
    let frag_uri = Url::from_file_path(&frag_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: frag_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: frag_text.to_string(),
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
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received query diagnostics");
        assert!(
            query_diag["diagnostics"].as_array().unwrap().is_empty(),
            "Query should be initially valid"
        );
    }
    let frag_text_new = "fragment UserFragRenamed on User { id }";
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: frag_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: frag_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received query diagnostics after change");
        let d_list = query_diag["diagnostics"].as_array().unwrap();
        assert!(
            !d_list.is_empty(),
            "Should have error after fragment rename"
        );
        assert!(
            d_list[0]["message"]
                .as_str()
                .unwrap()
                .contains("Unknown fragment: UserFrag")
        );
    }

    // Fix reference
    let query_text_new = "query { me { ...UserFragRenamed } }";
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

    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .unwrap();
        assert!(
            query_diag["diagnostics"].as_array().unwrap().is_empty(),
            "Diagnostics should clear after fixing reference"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(1000)]
async fn test_lsp_fragment_rename_cross_project() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir(&pkg_a).unwrap();
    fs::write(pkg_a.join("package.json"), "{}").unwrap();
    let schema_a_path = pkg_a.join("schema.graphql");
    fs::write(
        &schema_a_path,
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();
    let frag_path = pkg_a.join("frag.graphql");
    let frag_text = "fragment UserFrag on User @public { id }";
    fs::write(&frag_path, frag_text).unwrap();
    let pkg_b = base_dir.join("pkg_b");
    fs::create_dir(&pkg_b).unwrap();
    fs::write(pkg_b.join("package.json"), "{}").unwrap();
    let query_path = pkg_b.join("query.graphql");
    let query_text = "query { me { ...UserFrag } }";
    fs::write(&query_path, query_text).unwrap();
    let config = Config {
        output_dir: None,
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("pkg_a/schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_a/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
            ProjectConfig {
                schema: SchemaSource::Single("pkg_a/schema.graphql".to_string()),
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
        timeouts: None,
    };
    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            } else if msg.method() == "window/logMessage" {
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
    let frag_uri = Url::from_file_path(&frag_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: frag_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: frag_text.to_string(),
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
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received query diagnostics");
        assert!(
            query_diag["diagnostics"].as_array().unwrap().is_empty(),
            "Query should be initially valid"
        );
    }
    let frag_text_new = "fragment UserFragRenamed on User @public { id }";
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: frag_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: frag_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received query diagnostics after change");
        let d_list = query_diag["diagnostics"].as_array().unwrap();
        assert!(
            !d_list.is_empty(),
            "Should have error after cross-project fragment rename"
        );
        assert!(
            d_list[0]["message"]
                .as_str()
                .unwrap()
                .contains("Unknown fragment: UserFrag")
        );
    }

    // Fix reference in Project B
    let query_text_new = "query { me { ...UserFragRenamed } }";
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

    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .unwrap();
        assert!(
            query_diag["diagnostics"].as_array().unwrap().is_empty(),
            "Diagnostics should clear in other project after fixing reference"
        );
    }
}
