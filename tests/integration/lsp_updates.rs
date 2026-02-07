use crate::support::{self, lsp_did_open, lsp_initialize_sequence, lsp_send_notification, range};
use futures_util::StreamExt;
use graphql_rust::{
    config::{GlobPattern, ProjectConfig, SchemaSource},
    Config,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::lsp_types::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(1000)]
async fn test_lsp_fragment_collisions() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String } type Query { me: User }",
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
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
    ));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params_json = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                let params: PublishDiagnosticsParams = serde_json::from_value(params_json).unwrap();
                received_diags_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let uri_b = Url::from_file_path(&frag_b_path).unwrap();
    let uri_c = Url::from_file_path(&frag_c_path).unwrap();
    let uri_d = Url::from_file_path(&frag_d_path).unwrap();
    let uri_e = Url::from_file_path(&frag_e_path).unwrap();
    let uri_f = Url::from_file_path(&frag_f_path).unwrap();

    lsp_did_open(
        &mut service,
        uri_a.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_a_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        uri_b.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_b_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        uri_c.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_c_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        uri_d.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_d_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        uri_e.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_e_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        uri_f.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_f_path).unwrap(),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();

    // Check private collision in pkg_a
    let d_a = diags.get(&uri_a).unwrap();
    let diag = d_a.iter().find(|d| {
        d.message
            .contains("Duplicate fragment name: 'DuplicateFrag'")
    }).expect("Should find duplicate fragment diagnostic");
    assert_eq!(diag.range, range(0, 9, 0, 22));

    // Check shadowing in shadow.graphql
    let d_d = diags.get(&uri_d).unwrap();
    let diag = d_d.iter()
            .find(|d| d.message.contains("shadows a public fragment"))
            .expect("Should find shadowing hint");
    assert_eq!(diag.range, range(0, 9, 0, 19)); // fragment |PublicFrag

    // Check public collision
    let d_e = diags.get(&uri_e).unwrap();
    let diag = d_e.iter().find(|d| {
        d.message
            .contains("Duplicate public fragment name: 'PublicCollision'")
    }).expect("Should find public collision diagnostic");
    assert_eq!(diag.range, range(0, 9, 0, 24));
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
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { id name } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };
    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Wait for initial diagnostics
    tokio::time::sleep(Duration::from_millis(50)).await;
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
            uri: Url::from_file_path(fs::canonicalize(&schema_path).unwrap()).unwrap(),
            typ: FileChangeType::CHANGED,
        }],
    };
    lsp_send_notification(&mut service, "workspace/didChangeWatchedFiles", &params).await;

    // 6. Wait for diagnostics after schema change
    tokio::time::sleep(Duration::from_millis(50)).await;
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
    lsp_send_notification(&mut service, "textDocument/didChange", &params).await;

    // 8. Verify diagnostics cleared
    tokio::time::sleep(Duration::from_millis(50)).await;
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
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };
    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();
    let frag_uri = Url::from_file_path(fs::canonicalize(&frag_path).unwrap()).unwrap();

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
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
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: frag_uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: frag_text_new.to_string(),
        }],
    };
    lsp_send_notification(&mut service, "textDocument/didChange", &params).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
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
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: query_uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: query_text_new.to_string(),
        }],
    };
    lsp_send_notification(&mut service, "textDocument/didChange", &params).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
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
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };
    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();
    let frag_uri = Url::from_file_path(fs::canonicalize(&frag_path).unwrap()).unwrap();

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
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
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: frag_uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: frag_text_new.to_string(),
        }],
    };
    lsp_send_notification(&mut service, "textDocument/didChange", &params).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
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
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: query_uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: query_text_new.to_string(),
        }],
    };
    lsp_send_notification(&mut service, "textDocument/didChange", &params).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
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
