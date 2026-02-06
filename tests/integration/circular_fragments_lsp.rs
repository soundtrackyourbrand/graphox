use futures_util::StreamExt;
use graphql_rust::{Backend, Config};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
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
        base_dir: base_dir.clone(),
        projects: vec![graphql_rust::config::ProjectConfig {
            schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphql_rust::config::GlobPattern::Single("frags.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        lsp_automatic_codegen: Some(false),
        ..Config::default()
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track push diagnostics
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_push_diags_clone
                    .lock()
                    .unwrap()
                    .push(params.clone());
            }
        }
    });

    // Initialize
    let init_params = InitializeParams::default();
    let response = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // Open document
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

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(50)).await;

    let push_diags = received_push_diags.lock().unwrap();
    assert!(!push_diags.is_empty(), "Expected push diagnostics");

    let last = push_diags.last().unwrap();
    let diags = &last["diagnostics"];
    let found = diags.as_array().unwrap().iter().any(|d| {
        d["message"].as_str().unwrap_or("").contains("Circular fragment reference")
            || d["code"] == serde_json::json!({"value": "circular_fragment"})
    });
    assert!(found, "Expected circular fragment diagnostic in LSP push diags");
}
