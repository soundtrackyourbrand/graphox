use crate::support::{
    create_doc, create_initialized_lsp_service, create_lsp_service_with_socket, lsp_did_open,
    lsp_initialize_sequence, lsp_request_diagnostics, lsp_request_typed,
    make_temp_project_with_schema, write_project_file,
};
use futures_util::StreamExt;
use graphql_rust::config::RulesConfig;
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(10000)]
async fn test_duplicate_operation_names_cross_file() {
    let schema = "type User { id: ID! name: String! } type Query { user(id: ID!): User }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");

    // Enable the unique operation name rule for this test so duplicate
    // operation diagnostics are produced during validation.
    config.rules = Some(RulesConfig {
        required_fields: None,
        unique_operation_name: Some(true),
        no_duplicate_fields: None,
    });

    // Create service and capture server->client messages so we can assert push diagnostics
    let (mut service, mut messages) = create_lsp_service_with_socket(config);
    let received_push = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
        Url,
        Vec<Diagnostic>,
    >::new()));
    let received_push_clone = received_push.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params: PublishDiagnosticsParams = serde_json::from_value(
                    msg.get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .unwrap();
                received_push_clone
                    .lock()
                    .unwrap()
                    .insert(params.uri, params.diagnostics);
            }
        }
    });

    // create the query files before initialization so the workspace scan discovers them
    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    let query2_text = "query GetUser { user(id: \"2\") { id } }";
    let query1_uri = write_project_file(&tmpdir, "query1.graphql", query1_text);
    let query2_uri = write_project_file(&tmpdir, "query2.graphql", query2_text);

    lsp_initialize_sequence(&mut service).await;

    // Open the documents
    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Give some time for didOpen processing
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Request diagnostics for first file
    let params1 = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: query1_uri.clone(),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    // Poll for diagnostics until we find the duplicate diagnostic or timeout
    let mut found_dup = false;
    let mut last_diags_json: Option<String> = None;
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 2000 {
        let result1: DocumentDiagnosticReportResult =
            lsp_request_typed(&mut service, "textDocument/diagnostic", &params1).await;

        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
            result1
        {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            last_diags_json = Some(serde_json::to_string_pretty(&diagnostics).unwrap_or_default());
            if diagnostics.len() == 1 {
                let d = &diagnostics[0];
                if d.message.contains("Duplicate operation name 'GetUser'") {
                    found_dup = true;
                    let doc = create_doc(query1_uri.as_str(), query1_text);
                    assert_eq!(
                        d.range,
                        crate::support::range_for_token(&doc, query1_text, "GetUser")
                    );
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        found_dup,
        "Should find duplicate operation diagnostic in query1.graphql; last diagnostics: {:?}",
        last_diags_json
    );

    // Request diagnostics for second file
    let params2 = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: query2_uri.clone(),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    // Poll for diagnostics for query2 as well
    let mut found_dup2 = false;
    let mut last_diags_json2: Option<String> = None;
    let start2 = std::time::Instant::now();
    while start2.elapsed().as_millis() < 2000 {
        let result2: DocumentDiagnosticReportResult =
            lsp_request_typed(&mut service, "textDocument/diagnostic", &params2).await;

        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
            result2
        {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            last_diags_json2 = Some(serde_json::to_string_pretty(&diagnostics).unwrap_or_default());
            if diagnostics.len() == 1 {
                let d = &diagnostics[0];
                if d.message.contains("Duplicate operation name 'GetUser'") {
                    found_dup2 = true;
                    let doc = create_doc(query2_uri.as_str(), query2_text);
                    assert_eq!(
                        d.range,
                        crate::support::range_for_token(&doc, query2_text, "GetUser")
                    );
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        found_dup2,
        "Should find duplicate operation diagnostic in query2.graphql; last diagnostics: {:?}",
        last_diags_json2
    );
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_unique_operation_names_no_duplicates() {
    let schema = "type User { id: ID! name: String! } type Query { user(id: ID!): User }";
    let (tmpdir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create first file with GetUser operation
    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    let query1_uri = write_project_file(&tmpdir, "query1.graphql", query1_text);
    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;

    // Create second file with different operation name
    let query2_text = "query GetUserById { user(id: \"2\") { id } }";
    let query2_uri = write_project_file(&tmpdir, "query2.graphql", query2_text);
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Poll for diagnostics to ensure they are processed
    let mut diags_empty = false;
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 1000 {
        // Request diagnostics for first file using helper
        let result1 = lsp_request_diagnostics(&mut service, query1_uri.clone()).await;

        // Check that we got NO duplicate operation diagnostics
        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
            result1
        {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            let duplicate_diags: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.message.contains("Duplicate operation"))
                .collect();

            if duplicate_diags.is_empty() {
                diags_empty = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        diags_empty,
        "Should not find duplicate operation diagnostics when names are unique"
    );
}
