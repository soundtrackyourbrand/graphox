use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_diagnostics,
    make_temp_project_with_schema, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_did_open_reconciles_against_workspace_scan() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (tmpdir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    // 1. Create files BEFORE initialization so they are indexed by workspace scan
    let frag_text = "fragment MyFragment on User { id }";
    let query_text = "query MyQuery { user { ...MyFragment } }";
    let frag_uri = write_project_file(&tmpdir, "frag.graphql", frag_text);
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);

    // 2. Initialize LSP
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 3. Verify query is valid
    let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        assert!(
            full_report.full_document_diagnostic_report.items.is_empty(),
            "Query should be valid initially, but got: {:?}",
            full_report.full_document_diagnostic_report.items
        );
    }

    // 4. Open frag.graphql with content that REMOVES the fragment
    let frag_text_empty = "# No fragment here anymore";
    lsp_did_open(
        &mut service,
        frag_uri.clone(),
        "graphql",
        2,
        frag_text_empty,
    )
    .await;

    // 5. Verify query is now INVALID (MyFragment is missing)
    // We might need to wait a bit for background validation
    let mut found_error = false;
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 2000 {
        let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;
        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
            result
        {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            if diagnostics
                .iter()
                .any(|d| d.message.contains("Unknown fragment: MyFragment"))
            {
                found_error = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(
        found_error,
        "Query should show 'Unknown fragment' error after fragment was removed via didOpen"
    );
}
