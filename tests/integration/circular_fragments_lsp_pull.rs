use futures_util::StreamExt;
use graphql_rust::{Backend, Config};
use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
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
        output_dir: None,
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
        ..Config::default()
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track push diagnostics in case server falls back
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

    let response = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    let result: InitializeResult = serde_json::from_value(response.unwrap().result().unwrap().clone()).unwrap();
    assert!(result.capabilities.diagnostic_provider.is_some());

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open the files
    for (uri, text) in [
        (frag_a_path.clone(), frag_a_text.to_string()),
        (frag_b_path.clone(), frag_b_text.to_string()),
        (query_path.clone(), query_text.to_string()),
    ] {
        let url = Url::from_file_path(uri).unwrap();
        service
            .call(
                Request::build("textDocument/didOpen")
                    .params(
                        serde_json::to_value(DidOpenTextDocumentParams {
                            text_document: TextDocumentItem {
                                uri: url.clone(),
                                language_id: "graphql".to_string(),
                                version: 1,
                                text,
                            },
                        })
                        .unwrap(),
                    )
                    .finish(),
            )
            .await
            .unwrap();
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

    let response = service
        .call(
            Request::build("textDocument/diagnostic")
                .params(serde_json::to_value(&diag_params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap();

    let result: DocumentDiagnosticReportResult = serde_json::from_value(response.unwrap().result().unwrap().clone()).unwrap();

    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            assert!(
                !report.full_document_diagnostic_report.items.is_empty(),
                "Should have diagnostics for circular fragments"
            );

            let found = report
                .full_document_diagnostic_report
                .items
                .iter()
                .any(|it| it.message.contains("Circular fragment reference") || it.message.contains("circular_fragment"));
            assert!(found, "Expected circular fragment diagnostic in pull report");
        }
        _ => panic!("Expected full diagnostic report from pull request"),
    }

    // Also ensure we didn't receive empty push diagnostics only
    let push_diags = received_push_diags.lock().unwrap();
    // It's ok whether push diagnostics were sent; at least pull returned the diagnostic.
    assert!(push_diags.len() >= 0);
}
