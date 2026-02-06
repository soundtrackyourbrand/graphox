use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, RulesConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
#[ntest::timeout(10000)]
async fn test_duplicate_operation_names_cross_file() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
    )
    .unwrap();

    // Create both query files BEFORE initializing LSP so workspace scan finds them
    let query1_path = base_dir.join("query1.graphql");
    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    fs::write(&query1_path, query1_text).unwrap();

    let query2_path = base_dir.join("query2.graphql");
    let query2_text = "query GetUser { user(id: \"2\") { id } }";
    fs::write(&query2_path, query2_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        enable_schema_cache: Some(false),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Give the LSP time to complete workspace scan (this populates operation_names index)
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Canonicalize paths and create URIs
    let query1_path = std::fs::canonicalize(query1_path).unwrap();
    let query1_uri = Url::from_file_path(&query1_path).unwrap();

    let query2_path = std::fs::canonicalize(query2_path).unwrap();
    let query2_uri = Url::from_file_path(&query2_path).unwrap();

    // Open the documents (they're already scanned by workspace scan, but we need to open them for diagnostics)
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query1_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query1_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query2_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query2_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Give the LSP time to process diagnostics after didOpen
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

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

    let request1 = Request::build("textDocument/diagnostic")
        .id(2)
        .params(serde_json::to_value(&params1).unwrap())
        .finish();
    let response1 = service.call(request1).await.unwrap().unwrap();
    let result1: DocumentDiagnosticReportResult =
        serde_json::from_value(response1.result().unwrap().clone()).unwrap();

    // Check that we got a duplicate operation diagnostic for query1
    match result1 {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) => {
            let diagnostics = &full_report.full_document_diagnostic_report.items;

            let duplicate_diag = diagnostics
                .iter()
                .find(|d| d.message.contains("Duplicate operation name 'GetUser'"))
                .expect("Should find duplicate operation diagnostic in query1.graphql");

            assert_eq!(duplicate_diag.severity, Some(DiagnosticSeverity::ERROR));
            assert_eq!(
                duplicate_diag.code,
                Some(NumberOrString::String("duplicate_operation".to_string()))
            );
            // Should mention the other file
            assert!(duplicate_diag.message.contains("query2.graphql"));
        }
        _ => panic!("Expected full diagnostic report for query1"),
    }

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

    let request2 = Request::build("textDocument/diagnostic")
        .id(3)
        .params(serde_json::to_value(&params2).unwrap())
        .finish();
    let response2 = service.call(request2).await.unwrap().unwrap();
    let result2: DocumentDiagnosticReportResult =
        serde_json::from_value(response2.result().unwrap().clone()).unwrap();

    // Check that we got a duplicate operation diagnostic for query2
    match result2 {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) => {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            let duplicate_diag = diagnostics
                .iter()
                .find(|d| d.message.contains("Duplicate operation name 'GetUser'"))
                .expect("Should find duplicate operation diagnostic in query2.graphql");

            assert_eq!(duplicate_diag.severity, Some(DiagnosticSeverity::ERROR));
            // Should mention the other file
            assert!(duplicate_diag.message.contains("query1.graphql"));
        }
        _ => panic!("Expected full diagnostic report for query2"),
    }
}

#[tokio::test]
#[ntest::timeout(10000)]
async fn test_unique_operation_names_no_duplicates() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String! } type Query { user(id: ID!): User }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        rules: Some(RulesConfig {
            unique_operation_name: Some(true),
            ..RulesConfig::default()
        }),
        enable_schema_cache: Some(false),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Create first file with GetUser operation
    let query1_path = base_dir.join("query1.graphql");
    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    fs::write(&query1_path, query1_text).unwrap();
    let query1_path = std::fs::canonicalize(query1_path).unwrap();
    let query1_uri = Url::from_file_path(&query1_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query1_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query1_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Create second file with different operation name
    let query2_path = base_dir.join("query2.graphql");
    let query2_text = "query GetUserById { user(id: \"2\") { id } }";
    fs::write(&query2_path, query2_text).unwrap();
    let query2_path = std::fs::canonicalize(query2_path).unwrap();
    let query2_uri = Url::from_file_path(&query2_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query2_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query2_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Give the LSP some time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

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

    let request1 = Request::build("textDocument/diagnostic")
        .id(2)
        .params(serde_json::to_value(&params1).unwrap())
        .finish();
    let response1 = service.call(request1).await.unwrap().unwrap();
    let result1: DocumentDiagnosticReportResult =
        serde_json::from_value(response1.result().unwrap().clone()).unwrap();

    // Check that we got NO duplicate operation diagnostics
    match result1 {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) => {
            let diagnostics = &full_report.full_document_diagnostic_report.items;
            let duplicate_diags: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.message.contains("Duplicate operation"))
                .collect();

            assert!(
                duplicate_diags.is_empty(),
                "Should not find duplicate operation diagnostics when names are unique"
            );
        }
        _ => panic!("Expected full diagnostic report"),
    }
}
