use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_diagnostics, write_project_file,
};
use graphox::CodegenConfig;
use graphox::config::{Config, GlobPattern, ProjectConfig, SchemaSource};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_multiple_schemas_query_field_from_second_schema() {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let base_dir = dir.path().to_path_buf();

    // Schema 1
    let schema1 = "type Query { foo: String }";
    std::fs::write(base_dir.join("schema1.graphql"), schema1).expect("write schema1");

    // Schema 2
    let schema2 = "type Query { bar: String }";
    std::fs::write(base_dir.join("schema2.graphql"), schema2).expect("write schema2");

    std::fs::write(base_dir.join("package.json"), "{}").expect("write package.json");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Multiple(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ]))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Query using field 'bar' from second schema
    let text = "query { bar }";
    let query_uri = write_project_file(&dir, "query.graphql", text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, text).await;

    // Request diagnostics
    let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;

    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        let diagnostics = &full_report.full_document_diagnostic_report.items;

        for diag in diagnostics {
            if let Some(NumberOrString::String(code)) = &diag.code
                && code == "missing_field"
                && diag.message.contains("bar")
            {
                panic!("Incorrect missing_field error for 'bar': {}", diag.message);
            }
        }

        assert_eq!(
            diagnostics.len(),
            0,
            "Expected no diagnostics, but found: {:#?}",
            diagnostics
        );
    } else {
        panic!("Expected full diagnostic report");
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_multiple_schemas_with_schema_blocks() {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let base_dir = dir.path().to_path_buf();

    // Schema 1
    let schema1 = "schema { query: Query } type Query { foo: String }";
    std::fs::write(base_dir.join("schema1.graphql"), schema1).expect("write schema1");

    // Schema 2
    let schema2 = "schema { query: Query } type Query { bar: String }";
    std::fs::write(base_dir.join("schema2.graphql"), schema2).expect("write schema2");

    std::fs::write(base_dir.join("package.json"), "{}").expect("write package.json");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Multiple(vec![
                    "schema1.graphql".to_string(),
                    "schema2.graphql".to_string(),
                ]))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Query using field 'bar' from second schema
    let text = "query { bar }";
    let query_uri = write_project_file(&dir, "query.graphql", text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, text).await;

    // Request diagnostics
    let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;

    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        let diagnostics = &full_report.full_document_diagnostic_report.items;
        assert_eq!(
            diagnostics.len(),
            0,
            "Expected no diagnostics, but found: {:#?}",
            diagnostics
        );
    } else {
        panic!("Expected full diagnostic report");
    }
}
