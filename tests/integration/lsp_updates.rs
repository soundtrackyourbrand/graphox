use crate::support::{self, lsp_did_open, lsp_initialize_sequence, lsp_send_notification};
use futures_util::StreamExt;
use graphox::{
    Config,
    config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use std::sync::{Arc, Mutex};
use tower_lsp::lsp_types::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_lsp_fragment_collisions() {
    // Given: a workspace with multiple packages and fragments that will collide
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String } type Query { me: User }",
        )
        .with_file(
            "pkg_a/frag.graphql",
            "fragment DuplicateFrag on User { id }",
        )
        .with_file(
            "pkg_a/other.graphql",
            "fragment DuplicateFrag on User { name }",
        )
        .with_file(
            "pkg_b/pub.graphql",
            "fragment PublicFrag on User @public { id }",
        )
        .with_file(
            "pkg_a/shadow.graphql",
            "fragment PublicFrag on User { name }",
        )
        .with_file(
            "pkg_b/pub_collision.graphql",
            "fragment PublicCollision on User @public { id }",
        )
        .with_file(
            "pkg_a/pub_collision_2.graphql",
            "fragment PublicCollision on User @public { name }",
        );

    let base_dir = scenario.write_files().unwrap();

    // Use an explicit two-project config to match previous test semantics
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_a/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_b/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
    ));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params_json = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let params: PublishDiagnosticsParams = serde_json::from_value(params_json).unwrap();
                received_diags_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    // Open all files we wrote
    for rel in [
        "pkg_a/frag.graphql",
        "pkg_a/other.graphql",
        "pkg_b/pub.graphql",
        "pkg_a/shadow.graphql",
        "pkg_b/pub_collision.graphql",
        "pkg_a/pub_collision_2.graphql",
    ] {
        let path = base_dir.join(rel);
        let uri = Url::from_file_path(&path).unwrap();
        lsp_did_open(
            &mut service,
            uri,
            "graphql",
            1,
            &fs::read_to_string(&path).unwrap(),
        )
        .await;
    }

    // Wait for all diagnostics to be received
    let _ = support::wait_for_condition(|| received_diags.lock().unwrap().len() >= 6).await;

    let diags = received_diags.lock().unwrap();

    let uri_a = Url::from_file_path(base_dir.join("pkg_a/frag.graphql")).unwrap();
    let uri_d = Url::from_file_path(base_dir.join("pkg_a/shadow.graphql")).unwrap();
    let uri_e = Url::from_file_path(base_dir.join("pkg_b/pub_collision.graphql")).unwrap();

    // Check private collision in pkg_a
    let d_a = diags.get(&uri_a).unwrap();
    assert!(!d_a.is_empty());
    let diag = d_a
        .iter()
        .find(|d| {
            d.message
                .contains("Duplicate fragment name: 'DuplicateFrag'")
        })
        .expect("Should find our duplicate fragment diagnostic");
    let doc_a = crate::support::create_doc(
        uri_a.as_str(),
        &fs::read_to_string(base_dir.join("pkg_a/frag.graphql")).unwrap(),
    );
    assert_eq!(
        diag.range,
        crate::support::range_for_token(
            &doc_a,
            &fs::read_to_string(base_dir.join("pkg_a/frag.graphql")).unwrap(),
            "DuplicateFrag"
        )
    );

    // Check shadowing in shadow.graphql
    let d_d = diags.get(&uri_d).unwrap();
    assert!(!d_d.is_empty());
    let diag = d_d
        .iter()
        .find(|d| d.message.contains("shadows a public fragment"))
        .expect("Should find shadowing hint");
    assert_eq!(diag.severity, Some(DiagnosticSeverity::HINT));
    let doc_d = crate::support::create_doc(
        uri_d.as_str(),
        &fs::read_to_string(base_dir.join("pkg_a/shadow.graphql")).unwrap(),
    );
    assert_eq!(
        diag.range,
        crate::support::range_for_token(
            &doc_d,
            &fs::read_to_string(base_dir.join("pkg_a/shadow.graphql")).unwrap(),
            "PublicFrag"
        )
    );

    // Check public collision
    let d_e = diags.get(&uri_e).unwrap();
    assert!(!d_e.is_empty());
    let diag = d_e
        .iter()
        .find(|d| {
            d.message
                .contains("Duplicate public fragment name: 'PublicCollision'")
        })
        .expect("Should find public collision diagnostic");
    let doc_e = crate::support::create_doc(
        uri_e.as_str(),
        &fs::read_to_string(base_dir.join("pkg_b/pub_collision.graphql")).unwrap(),
    );
    assert_eq!(
        diag.range,
        crate::support::range_for_token(
            &doc_e,
            &fs::read_to_string(base_dir.join("pkg_b/pub_collision.graphql")).unwrap(),
            "PublicCollision"
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_lsp_diagnostics_on_schema_change() {
    // Given: a workspace with a schema and a query that initially matches
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String } type Query { me: User }",
        )
        .with_file("query.graphql", "query { me { id name } }");

    let base_dir = scenario.write_files().unwrap();
    let config = scenario.build_config(&base_dir);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();
    let query_text = "query { me { id name } }";
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Wait for initial diagnostics
    let _ = support::wait_for_condition_with_timeout(|| !received_diags.lock().unwrap().is_empty(), std::time::Duration::from_secs(10)).await;
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
    let schema_path = base_dir.join("schema.graphql");
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
    let _ = support::wait_for_condition_with_timeout(|| {
        let diags = received_diags.lock().unwrap();
        if diags.is_empty() {
            return false;
        }
        let last = diags.last().unwrap();
        !last["diagnostics"].as_array().unwrap().is_empty()
    }, std::time::Duration::from_secs(10))
    .await;
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
    let _ = support::wait_for_condition_with_timeout(|| {
        let diags = received_diags.lock().unwrap();
        if diags.is_empty() {
            return false;
        }
        let last = diags.last().unwrap();
        last["diagnostics"].as_array().unwrap().is_empty()
    }, std::time::Duration::from_secs(10))
    .await;
    {
        let diags = received_diags.lock().unwrap();
        let last = diags.last().unwrap();
        assert!(last["diagnostics"].as_array().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(10000)]
async fn test_lsp_fragment_rename_same_project() {
    // Given: a simple project with a fragment and a query that references it
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String } type Query { me: User }",
        )
        .with_file("frag.graphql", "fragment UserFrag on User { id }")
        .with_file("query.graphql", "query { me { ...UserFrag } }");

    let base_dir = scenario.write_files().unwrap();

    // Use an explicit config that includes all graphql files
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let frag_path = base_dir.join("frag.graphql");
    let query_path = base_dir.join("query.graphql");
    let frag_uri = Url::from_file_path(fs::canonicalize(&frag_path).unwrap()).unwrap();
    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();

    lsp_did_open(
        &mut service,
        frag_uri.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        query_uri.clone(),
        "graphql",
        1,
        &fs::read_to_string(&query_path).unwrap(),
    )
    .await;

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        diags.iter().any(|d| d["uri"] == query_uri.as_str())
    })
    .await;

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

    // Rename fragment
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

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        if let Some(d) = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()) {
            !d["diagnostics"].as_array().unwrap().is_empty()
        } else {
            false
        }
    })
    .await;

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

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        if let Some(d) = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()) {
            d["diagnostics"].as_array().unwrap().is_empty()
        } else {
            false
        }
    })
    .await;

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
#[ntest::timeout(10000)]
async fn test_lsp_fragment_rename_cross_project() {
    // Given: two packages A and B where A exports a public fragment used by B
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("pkg_a/package.json", "{}")
        .with_file(
            "pkg_a/schema.graphql",
            "type User { id: ID! } type Query { me: User }",
        )
        .with_file(
            "pkg_a/frag.graphql",
            "fragment UserFrag on User @public { id }",
        )
        .with_file("pkg_b/package.json", "{}")
        .with_file("pkg_b/query.graphql", "query { me { ...UserFrag } }");

    let base_dir = scenario.write_files().unwrap();

    // Explicit two-project config like the original test
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("pkg_a/schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_a/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("pkg_a/schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_b/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let frag_path = base_dir.join("pkg_a/frag.graphql");
    let query_path = base_dir.join("pkg_b/query.graphql");
    let frag_uri = Url::from_file_path(fs::canonicalize(&frag_path).unwrap()).unwrap();
    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();

    lsp_did_open(
        &mut service,
        frag_uri.clone(),
        "graphql",
        1,
        &fs::read_to_string(&frag_path).unwrap(),
    )
    .await;
    lsp_did_open(
        &mut service,
        query_uri.clone(),
        "graphql",
        1,
        &fs::read_to_string(&query_path).unwrap(),
    )
    .await;

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        diags.iter().any(|d| d["uri"] == query_uri.as_str())
    })
    .await;

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

    // Rename fragment in project A
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

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        if let Some(d) = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()) {
            !d["diagnostics"].as_array().unwrap().is_empty()
        } else {
            false
        }
    })
    .await;

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

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        if let Some(d) = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()) {
            d["diagnostics"].as_array().unwrap().is_empty()
        } else {
            false
        }
    })
    .await;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_lsp_diagnostics_cleared_after_fix() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String } type Query { me: User }",
        )
        .with_file("query.graphql", "query { me { id id } }");

    let base_dir = scenario.write_files().unwrap();

    let config = graphox::Config::new_test(
        base_dir.clone(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                ))
                .with_codegen(graphox::config::CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false)
    .with_rules(graphox::config::RulesConfig::default().with_no_duplicate_fields(true));

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();
    let query_text = "query { me { id id } }";
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        diags.iter().any(|d| {
            d["uri"] == query_uri.as_str() && !d["diagnostics"].as_array().unwrap().is_empty()
        })
    })
    .await;

    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received diagnostics for query");
        let d_list = query_diag["diagnostics"].as_array().unwrap();
        assert!(!d_list.is_empty(), "Should have duplicate field error");
        assert!(
            d_list[0]["message"]
                .as_str()
                .unwrap()
                .contains("Duplicate field"),
            "Error message should mention duplicate field"
        );
    }

    let query_text_fixed = "query { me { id } }";
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

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        if let Some(d) = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()) {
            d["diagnostics"].as_array().unwrap().is_empty()
        } else {
            false
        }
    })
    .await;

    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received cleared diagnostics");
        assert!(
            query_diag["diagnostics"].as_array().unwrap().is_empty(),
            "Diagnostics should be cleared after fixing duplicate field"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_lsp_fragment_error_cleared_after_fix() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String } type Query { me: User }",
        )
        .with_file("query.graphql", "query { me { ...UserFields } }");

    let base_dir = scenario.write_files().unwrap();

    let config = graphox::Config::new_test(
        base_dir.clone(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                ))
                .with_codegen(graphox::config::CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_uri = Url::from_file_path(fs::canonicalize(&query_path).unwrap()).unwrap();
    let query_text = "query { me { ...UserFields } }";
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        diags.iter().any(|d| {
            d["uri"] == query_uri.as_str() && !d["diagnostics"].as_array().unwrap().is_empty()
        })
    })
    .await;

    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received diagnostics for query");
        let d_list = query_diag["diagnostics"].as_array().unwrap();
        assert!(!d_list.is_empty(), "Should have unknown fragment error");
        assert!(
            d_list[0]["message"]
                .as_str()
                .unwrap()
                .contains("Unknown fragment"),
            "Error message should mention unknown fragment"
        );
    }

    let fragment_path = base_dir.join("fragment.graphql");
    let fragment_text = "fragment UserFields on User { id name }";

    fs::write(&fragment_path, fragment_text).unwrap();
    let fragment_uri = Url::from_file_path(fs::canonicalize(&fragment_path).unwrap()).unwrap();

    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        fragment_text,
    )
    .await;

    let _ = support::wait_for_condition(|| {
        let diags = received_diags.lock().unwrap();
        if let Some(d) = diags.iter().rev().find(|d| d["uri"] == query_uri.as_str()) {
            d["diagnostics"].as_array().unwrap().is_empty()
        } else {
            false
        }
    })
    .await;

    {
        let diags = received_diags.lock().unwrap();
        let query_diag = diags
            .iter()
            .rev()
            .find(|d| d["uri"] == query_uri.as_str())
            .expect("Should have received cleared diagnostics");
        assert!(
            query_diag["diagnostics"].as_array().unwrap().is_empty(),
            "Diagnostics should be cleared after creating the fragment"
        );
    }
}
