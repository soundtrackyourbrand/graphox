use crate::support::{self, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use graphox::{
    config::{GlobPattern, ProjectConfig, SchemaSource},
    Config,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp_server::ls_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(3000)]
async fn test_lsp_duplicate_fragments_same_project_via_config() {
    // Given: two packages each with a fragment that share the same name
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema.graphql", "type Query { me: User } type User { id: ID! name: String }")
        .with_file("pkg_a/frag_a.graphql", "fragment DuplicateFrag on User { id }")
        .with_file("pkg_b/frag_b.graphql", "fragment DuplicateFrag on User { name }");

    let base_dir = scenario.write_files().unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            emit_permission_data: None,
            codegen: Some(CodegenConfig::disabled()),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let mut initialized = scenario.with_config(config).initialize().await;
    let service = initialized.service();

    let frag_a = base_dir.join("pkg_a/frag_a.graphql");
    let uri_a = graphox::utils::path_to_uri(&frag_a).unwrap();

    // Pull diagnostics for frag_a and assert duplicate fragment diagnostic exists
    let params = tower_lsp_server::ls_types::DocumentDiagnosticParams {
        text_document: tower_lsp_server::ls_types::TextDocumentIdentifier { uri: uri_a.clone() },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: tower_lsp_server::ls_types::DocumentDiagnosticReportResult =
        crate::support::lsp_request_typed(service, "textDocument/diagnostic", &params).await;

    if let tower_lsp_server::ls_types::DocumentDiagnosticReportResult::Report(
        tower_lsp_server::ls_types::DocumentDiagnosticReport::Full(full_report),
    ) = result
    {
        let diagnostics = &full_report.full_document_diagnostic_report.items;
        assert!(diagnostics.iter().any(|d| d.message.contains("Duplicate fragment name: 'DuplicateFrag'")));
    } else {
        panic!("Expected full diagnostic report for frag_a");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(3000)]
async fn test_lsp_private_duplicates_different_projects_no_error() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema.graphql", "type Query { me: User } type User { id: ID! name: String }")
        .with_file("pkg_a/frag_a.graphql", "fragment DuplicateFrag on User { id }")
        .with_file("pkg_b/frag_b.graphql", "fragment DuplicateFrag on User { name }");

    let base_dir = scenario.write_files().unwrap();

    let frag_a_path = base_dir.join("pkg_a/frag_a.graphql");
    let frag_b_path = base_dir.join("pkg_b/frag_b.graphql");

    let config = Config {
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_a/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                emit_permission_data: None,
                codegen: Some(CodegenConfig::disabled()),
            },
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_b/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                emit_permission_data: None,
                codegen: Some(CodegenConfig::disabled()),
            },
        ],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Uri, Vec<Diagnostic>>::new(),
    ));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params: PublishDiagnosticsParams = serde_json::from_value(
                    msg.get("params").cloned().unwrap_or(serde_json::Value::Null),
                )
                .unwrap();
                received_diags_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let uri_a = graphox::utils::path_to_uri(&frag_a_path).unwrap();
    let uri_b = graphox::utils::path_to_uri(&frag_b_path).unwrap();

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

    tokio::time::sleep(Duration::from_millis(50)).await;

    let diags = received_diags.lock().unwrap();

    let d_a = diags.get(&uri_a).unwrap();
    assert!(!d_a
        .iter()
        .any(|d| d.message.contains("Duplicate fragment name: 'DuplicateFrag'")));
}
