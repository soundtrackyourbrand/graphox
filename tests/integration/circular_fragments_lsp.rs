use crate::support::{self, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use graphql_rust::Config;
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_circular_fragment_diagnostic() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let frag_path = base_dir.join("frags.graphql");
    let frag_text = "fragment FragA on User { ...FragB }\nfragment FragB on User { ...FragA }";
    fs::write(&frag_path, frag_text).unwrap();

    let config = Config {
        projects: vec![graphql_rust::config::ProjectConfig {
            schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphql_rust::config::GlobPattern::Single("frags.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    // Track push diagnostics
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
                received_push_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    // Open document
    let frag_uri = Url::from_file_path(&frag_path).unwrap();
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(50)).await;

    let push_diags = received_push_diags.lock().unwrap();
    assert!(!push_diags.is_empty(), "Expected push diagnostics");

    let last = push_diags.last().unwrap();
    let diags = last["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 2);
    
    let doc = crate::support::create_doc(frag_uri.as_str(), frag_text);

    // Diag 1: FragB in FragA (line 0)
    let diag1 = diags.iter().find(|d| d["range"]["start"]["line"] == 0).unwrap();
    assert!(diag1["message"].as_str().unwrap().contains("Circular fragment reference"));
    let expected1 = crate::support::range_for_token(&doc, frag_text, "FragB");
    assert_eq!(diag1["range"]["start"]["character"], expected1.start.character);
    assert_eq!(diag1["range"]["end"]["character"], expected1.end.character);

    // Diag 2: FragA in FragB (line 1)
    let diag2 = diags.iter().find(|d| d["range"]["start"]["line"] == 1).unwrap();
    assert!(diag2["message"].as_str().unwrap().contains("Circular fragment reference"));
    let expected2 = crate::support::range(1, 28, 1, 33);
    assert_eq!(diag2["range"]["start"]["character"], expected2.start.character);
    assert_eq!(diag2["range"]["end"]["character"], expected2.end.character);
}