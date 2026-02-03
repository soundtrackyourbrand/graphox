use futures_util::StreamExt;
use graphql_rust::{Backend, Config, config::ProjectConfig, config::SchemaSource, config::GlobPattern};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        base_dir: base_dir.to_path_buf(),
    };
    let client_capture = Arc::new(Mutex::new(None));
    let client_capture_clone = client_capture.clone();
    let (mut service, mut messages) = LspService::new(|client| {
        let mut cap = client_capture_clone.lock().unwrap();
        *cap = Some(client.clone());
        Backend::new(client, Some(config), schema_path.to_str().unwrap())
    });
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
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
    tokio::time::sleep(Duration::from_millis(200)).await;
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
    tokio::time::sleep(Duration::from_millis(500)).await;
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
    tokio::time::sleep(Duration::from_millis(200)).await;
    {
        let diags = received_diags.lock().unwrap();
        let last = diags.last().unwrap();
        assert!(last["diagnostics"].as_array().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_lsp_fragment_rename_same_project() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type User { id: ID! name: String } type Query { me: User }").unwrap();
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
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        base_dir: base_dir.clone(),
    };
    let (mut service, mut messages) = LspService::new(|client| {
        Backend::new(client, Some(config), schema_path.to_str().unwrap())
    });
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            println!("MSG: {}", msg.method());
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
        println!("Background task exited");
    });
    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();
    let frag_uri = Url::from_file_path(&frag_path).unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem { uri: frag_uri.clone(), language_id: "graphql".to_string(), version: 1, text: frag_text.to_string() }
    }).unwrap()).finish()).await.unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem { uri: query_uri.clone(), language_id: "graphql".to_string(), version: 1, text: query_text.to_string() }
    }).unwrap()).finish()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()).expect("Should have received query diagnostics");
        assert!(query_diag["diagnostics"].as_array().unwrap().is_empty(), "Query should be initially valid");
    }
    let frag_text_new = "fragment UserFragRenamed on User { id }";
    service.call(Request::build("textDocument/didChange").params(serde_json::to_value(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri: frag_uri.clone(), version: 2 },
        content_changes: vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: frag_text_new.to_string() }]
    }).unwrap()).finish()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()).expect("Should have received query diagnostics after change");
        let d_list = query_diag["diagnostics"].as_array().unwrap();
        assert!(!d_list.is_empty(), "Should have error after fragment rename");
        assert!(d_list[0]["message"].as_str().unwrap().contains("Unknown fragment: UserFrag"));
    }

    // Fix reference
    let query_text_new = "query { me { ...UserFragRenamed } }";
    service.call(Request::build("textDocument/didChange").params(serde_json::to_value(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri: query_uri.clone(), version: 2 },
        content_changes: vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: query_text_new.to_string() }]
    }).unwrap()).finish()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()).unwrap();
        assert!(query_diag["diagnostics"].as_array().unwrap().is_empty(), "Diagnostics should clear after fixing reference");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_lsp_fragment_rename_cross_project() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir(&pkg_a).unwrap();
    fs::write(pkg_a.join("package.json"), "{}").unwrap();
    let schema_a_path = pkg_a.join("schema.graphql");
    fs::write(&schema_a_path, "type User { id: ID! } type Query { me: User }").unwrap();
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
            },
            ProjectConfig {
                schema: SchemaSource::Single("pkg_a/schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_b/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
            }
        ],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        base_dir: base_dir.clone(),
    };
    let (mut service, mut messages) = LspService::new(|client| {
        Backend::new(client, Some(config), schema_a_path.to_str().unwrap())
    });
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            println!("MSG: {}", msg.method());
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
        println!("Background task exited");
    });
    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();
    let frag_uri = Url::from_file_path(&frag_path).unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem { uri: frag_uri.clone(), language_id: "graphql".to_string(), version: 1, text: frag_text.to_string() }
    }).unwrap()).finish()).await.unwrap();
    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem { uri: query_uri.clone(), language_id: "graphql".to_string(), version: 1, text: query_text.to_string() }
    }).unwrap()).finish()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()).expect("Should have received query diagnostics");
        assert!(query_diag["diagnostics"].as_array().unwrap().is_empty(), "Query should be initially valid");
    }
    let frag_text_new = "fragment UserFragRenamed on User @public { id }";
    service.call(Request::build("textDocument/didChange").params(serde_json::to_value(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri: frag_uri.clone(), version: 2 },
        content_changes: vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: frag_text_new.to_string() }]
    }).unwrap()).finish()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()).expect("Should have received query diagnostics after change");
        let d_list = query_diag["diagnostics"].as_array().unwrap();
        assert!(!d_list.is_empty(), "Should have error after cross-project fragment rename");
        assert!(d_list[0]["message"].as_str().unwrap().contains("Unknown fragment: UserFrag"));
    }

    // Fix reference in Project B
    let query_text_new = "query { me { ...UserFragRenamed } }";
    service.call(Request::build("textDocument/didChange").params(serde_json::to_value(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri: query_uri.clone(), version: 2 },
        content_changes: vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: query_text_new.to_string() }]
    }).unwrap()).finish()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()).unwrap();
        assert!(query_diag["diagnostics"].as_array().unwrap().is_empty(), "Diagnostics should clear in other project after fixing reference");
    }
}
