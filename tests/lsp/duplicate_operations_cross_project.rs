use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_diagnostics, write_project_file,
};
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, RulesConfig, SchemaSource};
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::*;

#[tokio::test]
async fn test_duplicate_operation_names_across_projects_same_schema() {
    let dir = TempDir::new().expect("failed to create tempdir");
    let schema_text = "type Query { user: User } type User { id: ID! }";
    let schema_path = dir.path().join("schema.graphql");
    fs::write(&schema_path, schema_text).expect("write schema");
    fs::write(dir.path().join("package.json"), "{}").expect("write package.json");

    // Create files BEFORE initialization so workspace scan finds them
    let query1_text = "query GetUser { user { id } }";
    let query2_text = "query GetUser { user { id } }";
    let query1_uri = write_project_file(&dir, "pkg1/query1.graphql", query1_text);
    let query2_uri = write_project_file(&dir, "pkg2/query2.graphql", query2_text);

    // Define two projects pointing to the SAME schema but different include paths
    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg1".to_string()))
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg2".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_rules(RulesConfig::default().with_unique_operation_name(true))
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config.clone()).await;

    // Open them
    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Request diagnostics for first file
    let mut diagnostics = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 3000 {
        let result1 = lsp_request_diagnostics(&mut service, query1_uri.clone()).await;
        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
            result1
        {
            diagnostics = full_report.full_document_diagnostic_report.items;
            if diagnostics
                .iter()
                .any(|d| d.message.contains("Duplicate operation"))
            {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let has_dup = diagnostics
        .iter()
        .any(|d| d.message.contains("Duplicate operation name 'GetUser'"));
    assert!(
        !has_dup,
        "FIX VERIFIED: LSP should NOT report duplicate operation name 'GetUser' across project boundaries when sharing a schema. Diagnostics: {:?}",
        diagnostics
    );
}
