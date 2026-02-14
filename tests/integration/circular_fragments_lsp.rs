use crate::support::{self, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use graphox::Config;
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_circular_fragment_diagnostic() {
    // Given: a file containing two fragments that reference each other
    let frag_text = "fragment FragA on User { ...FragB }\nfragment FragB on User { ...FragA }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema.graphql", "type Query { me: User } type User { id: ID! name: String }")
        .with_file("frags.graphql", frag_text);

    let base_dir = scenario.write_files().unwrap();

    let config = Config {
        projects: vec![graphox::config::ProjectConfig {
            schema: graphox::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphox::config::GlobPattern::Single("frags.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            codegen: Some(CodegenConfig::disabled()),
        }],
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut initialized) = scenario.with_config(config).initialize().await;
    let service = initialized.service();

    // Open document
    let frag_uri = initialized.uri_for("frags.graphql");

    // Collect push diagnostics using the helper DiagnosticCollector
    let mut receiver = tokio::spawn(async move { /* noop - socket consumed by LspTestScenario */ });

    // The scenario already opens files during initialize; we only need to wait
    // a short time for validation to run.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Pull diagnostics via document diagnostic request to assert counts and ranges
    let params = tower_lsp::lsp_types::DocumentDiagnosticParams {
        text_document: tower_lsp::lsp_types::TextDocumentIdentifier { uri: frag_uri.clone() },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: tower_lsp::lsp_types::DocumentDiagnosticReportResult =
        crate::support::lsp_request_typed(service, "textDocument/diagnostic", &params).await;

    if let tower_lsp::lsp_types::DocumentDiagnosticReportResult::Report(
        tower_lsp::lsp_types::DocumentDiagnosticReport::Full(full_report),
    ) = result
    {
        let diagnostics = &full_report.full_document_diagnostic_report.items;
        assert_eq!(diagnostics.len(), 2);

        let doc = crate::support::create_doc(frag_uri.as_str(), frag_text);

        // Diag 1: FragB in FragA (line 0)
        let diag1 = diagnostics.iter().find(|d| d.range.start.line == 0).unwrap();
        assert!(diag1.message.contains("Circular fragment reference"));
        let expected1 = crate::support::range_for_token(&doc, frag_text, "FragB");
        assert_eq!(diag1.range.start.character, expected1.start.character);
        assert_eq!(diag1.range.end.character, expected1.end.character);

        // Diag 2: FragA in FragB (line 1)
        let diag2 = diagnostics.iter().find(|d| d.range.start.line == 1).unwrap();
        assert!(diag2.message.contains("Circular fragment reference"));
        let expected2 = crate::support::range_for_token_at_index(&doc, frag_text, "FragA", 1);
        assert_eq!(diag2.range.start.character, expected2.start.character);
        assert_eq!(diag2.range.end.character, expected2.end.character);
    } else {
        panic!("Unexpected diagnostic result: {:?}", result);
    }
