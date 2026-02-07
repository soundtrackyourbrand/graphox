use crate::support::{self, lsp_did_open, lsp_request_typed};
use futures_util::StreamExt;
use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphql_rust::Config;
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_pull_circular_fragments() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let frag_a_path = base_dir.join("frag_a.graphql");
    let frag_a_text = "fragment FragA on User { ...FragB }";
    fs::write(&frag_a_path, frag_a_text).unwrap();

    let frag_b_path = base_dir.join("frag_b.graphql");
    let frag_b_text = "fragment FragB on User { ...FragA }";
    fs::write(&frag_b_path, frag_b_text).unwrap();

    // Create a query that uses FragA so fragments are considered
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { ...FragA } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
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

    // Track push diagnostics in case server falls back
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

    // Initialize with pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;
    assert!(result.capabilities.diagnostic_provider.is_some());

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open the files
    for (path, text) in [
        (&frag_a_path, frag_a_text),
        (&frag_b_path, frag_b_text),
        (&query_path, query_text),
    ] {
        let url = Url::from_file_path(path).unwrap();
        lsp_did_open(&mut service, url, "graphql", 1, text).await;
    }

    // Wait for validation
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Request diagnostics via pull for query file
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(Url::from_file_path(&query_path).unwrap()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            let items = &report.full_document_diagnostic_report.items;
            // query.graphql uses FragA, which starts a cycle.
            // Expected diagnostics: 1 (on the spread ...FragA in query.graphql)
            assert_eq!(items.len(), 1);
            let diag = &items[0];
            
            assert!(diag.message.contains("Circular fragment reference"));
            
            let doc = crate::support::create_doc(diag_params.text_document.uri.as_str(), query_text);
            assert_eq!(diag.range, crate::support::range_for_token(&doc, query_text, "FragA"));
        }
        _ => panic!("Expected full diagnostic report from pull request"),
    }
}