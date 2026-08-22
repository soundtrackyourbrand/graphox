use crate::support::{
    create_initialized_lsp_service, create_lsp_service_with_socket, create_service, lsp_did_close,
    lsp_did_open, lsp_request_diagnostics, lsp_request_typed, make_temp_project_with_schema,
    write_project_file,
};
use ahash::AHashMap;
use futures_util::{SinkExt, StreamExt};
use graphox::{
    Backend, Config,
    config::{
        CodegenConfig, GlobPattern, ProjectConfig, RequiredFieldRule, RulesConfig, SchemaSource,
        TimeoutConfig,
    },
};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::Response;
use tower_lsp_server::ls_types::*;
use tower_service::Service;

async fn wait_for_workspace_loaded(
    service: &mut tower_lsp_server::LspService<crate::support::LspBackend>,
) {
    let backend = service.inner();
    let start = tokio::time::Instant::now();
    while !backend.workspace_loaded.load(Ordering::SeqCst) {
        assert!(
            start.elapsed() <= Duration::from_secs(10),
            "Timed out waiting for workspace scan to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_basic() {
    let query_text = "query GetUser { user { nonExistentField } }"; // Invalid field
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query_path = base_dir.join("query.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = create_lsp_service_with_socket(config);

    // Track push diagnostics (should not receive any when using pull)
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
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
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result: InitializeResult =
        lsp_request_typed(&mut service, "initialize", &init_params).await;
    assert!(result.capabilities.diagnostic_provider.is_some());

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Request diagnostics via pull
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    // Should return full diagnostic report
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            assert!(
                !report.full_document_diagnostic_report.items.is_empty(),
                "Should have diagnostics for invalid field"
            );
            assert!(
                report.full_document_diagnostic_report.result_id.is_some(),
                "Should have result_id"
            );

            // Verify diagnostic mentions the field
            let diag_msg = &report.full_document_diagnostic_report.items[0].message;
            assert!(diag_msg.contains("nonExistentField") || diag_msg.contains("field"));
        }
        _ => panic!("Expected full diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostic_refresh_survives_delayed_client_response() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        )
        .with_file("query.graphql", "query GetUser { user { id } }");

    let base_dir = scenario.write_files().unwrap();
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, socket) = LspService::new(move |client| {
        graphox::GraphoxLanguageServer::new(Backend::new(client, config))
    });
    let (mut requests, mut responses) = socket.split();
    let (refresh_responded_tx, refresh_responded_rx) = tokio::sync::oneshot::channel();

    let response_task = tokio::spawn(async move {
        while let Some(request) = requests.next().await {
            if request.method() != "workspace/diagnostic/refresh" {
                continue;
            }

            let request_id = request
                .id()
                .cloned()
                .expect("workspace/diagnostic/refresh should include a request id");

            tokio::time::sleep(Duration::from_millis(650)).await;

            responses
                .send(Response::from_ok(request_id, serde_json::Value::Null))
                .await
                .expect("failed to send delayed workspace/diagnostic/refresh response");

            refresh_responded_tx
                .send(())
                .expect("refresh response receiver dropped unexpectedly");
            return;
        }

        panic!("workspace/diagnostic/refresh was never requested");
    });

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    tokio::time::timeout(Duration::from_secs(3), refresh_responded_rx)
        .await
        .expect("timed out waiting to send delayed workspace/diagnostic/refresh response")
        .expect("workspace/diagnostic/refresh response task dropped unexpectedly");

    response_task
        .await
        .expect("workspace/diagnostic/refresh response task panicked");

    let _: Option<Vec<SymbolInformation>> = lsp_request_typed(
        &mut service,
        "workspace/symbol",
        &WorkspaceSymbolParams {
            query: "GetUser".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_returns_empty_for_unconfigured_file() {
    let configured_text = "query Configured { user { id } }";
    let ignored_text = "query Ignored { user { invalidField } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("configured.graphql", configured_text)
        .with_file("ignored.graphql", ignored_text);

    let base_dir = scenario.write_files().unwrap();
    let ignored_path = base_dir.join("ignored.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("configured.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let ignored_uri = graphox::utils::path_to_uri(&ignored_path).unwrap();
    lsp_did_open(
        &mut service,
        ignored_uri.clone(),
        "graphql",
        1,
        ignored_text,
    )
    .await;

    let result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(ignored_uri),
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            assert!(
                report.full_document_diagnostic_report.items.is_empty(),
                "Unconfigured files should return empty pull diagnostics"
            );
            assert!(
                report.full_document_diagnostic_report.result_id.is_some(),
                "Unconfigured files should still return a result_id"
            );
        }
        _ => panic!("Expected full diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_unchanged() {
    let query_text = "query GetUser { user { id name } }"; // Valid query
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query_path = base_dir.join("query.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    // Initialize with pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // First pull request
    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first_result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    let result_id = match first_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.result_id.clone()
        }
        _ => panic!("Expected full diagnostic report on first request"),
    };

    // Second pull request with same result_id - document unchanged
    let diag_params2 = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: result_id,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let second_result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params2).await;

    // Should return unchanged report
    match second_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => {
            // Success - document unchanged
        }
        _ => panic!("Expected unchanged diagnostic report when document hasn't changed"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_refresh_after_duplicate_file_deleted_and_closed() {
    let schema = "type User { id: ID! name: String! } type Query { user(id: ID!): User }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_unique_operation_name(true));

    let query1_text = "query GetUser { user(id: \"1\") { id name } }";
    let query2_text = "query GetUser { user(id: \"2\") { id } }";
    let query1_uri = write_project_file(&tmpdir, "query1.graphql", query1_text);
    let query2_uri = write_project_file(&tmpdir, "query2.graphql", query2_text);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    let first_result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(query1_uri.clone()),
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    let previous_result_id = match first_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            let diagnostics = &report.full_document_diagnostic_report.items;
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.message.contains("Duplicate operation name 'GetUser'")),
                "Expected duplicate operation diagnostic before deleting the second file: {diagnostics:#?}"
            );
            report
                .full_document_diagnostic_report
                .result_id
                .expect("Expected initial result_id")
        }
        _ => panic!("Expected full diagnostic report before deletion"),
    };

    let query2_path = tmpdir.path().join("query2.graphql");
    std::fs::remove_file(&query2_path).expect("delete duplicate query file");
    lsp_did_close(&mut service, query2_uri).await;

    let refreshed_result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(query1_uri),
            identifier: None,
            previous_result_id: Some(previous_result_id.clone()),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match refreshed_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            let refreshed_result_id = report
                .full_document_diagnostic_report
                .result_id
                .expect("Expected refreshed result_id after delete");
            let diagnostics = report.full_document_diagnostic_report.items;
            assert_ne!(refreshed_result_id, previous_result_id);
            assert!(
                diagnostics
                    .iter()
                    .all(|d| !d.message.contains("Duplicate operation name 'GetUser'")),
                "Duplicate operation diagnostic should clear after the duplicate file is deleted: {diagnostics:#?}"
            );
        }
        _ => panic!("Expected refreshed full diagnostic report after deletion"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_required_field_satisfied_by_unopened_embedded_fragment() {
    let schema = r#"
        type Query { editorialHome: EditorialHome! }
        type EditorialHome { sections: EditorialSectionConnection! }
        type EditorialSectionConnection { edges: [EditorialSectionEdge!]! }
        type EditorialSectionEdge { node: EditorialSection! }
        type EditorialSection { id: ID! title: String! }
    "#;

    let fragment_text = r#"import { graphql } from 'app/graphql'

export const EditorialSectionFragmentDoc = graphql(/* GraphQL */ `
  fragment EditorialSection on EditorialSection {
    id
    title
  }
`)
"#;

    let query_text = r#"import { graphql } from 'app/graphql'

export const EditorialHomeDoc = graphql(/* GraphQL */ `
  query EditorialHome {
    editorialHome {
      sections {
        edges {
          node {
            ...EditorialSection
          }
        }
      }
    }
  }
`)
"#;

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let (dir, mut config) = crate::support::make_temp_project_with_schema(schema, "**/*.ts");
    write_project_file(&dir, "components/editorial/fragments.ts", fragment_text);
    let query_uri = write_project_file(&dir, "navigation/home-data.ts", query_text);

    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));

    let (mut service, _handle) = create_initialized_lsp_service(config).await;
    wait_for_workspace_loaded(&mut service).await;
    lsp_did_open(&mut service, query_uri.clone(), "typescript", 1, query_text).await;

    let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;

    let diagnostics = match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.items
        }
        _ => panic!("Expected full diagnostic report"),
    };

    let required_field_diags: Vec<_> = diagnostics
        .iter()
        .filter(|diag| {
            diag.code == Some(NumberOrString::String("required_field_missing".to_string()))
        })
        .collect();

    assert!(
        required_field_diags.is_empty(),
        "Fragment spread should satisfy required field checks in embedded cross-file diagnostics. Diagnostics: {:?}",
        diagnostics
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_waits_for_workspace_fragment_context() {
    let schema = r#"
        type Query { editorialHome: EditorialHome! }
        type EditorialHome { sections: EditorialSectionConnection! }
        type EditorialSectionConnection { edges: [EditorialSectionEdge!]! }
        type EditorialSectionEdge { node: EditorialSection! }
        type EditorialSection { id: ID! title: String! }
    "#;

    let (dir, mut config) = crate::support::make_temp_project_with_schema(schema, "**/*.ts");

    let fragment_text = r#"import { graphql } from 'app/graphql'

export const EditorialSectionFragmentDoc = graphql(/* GraphQL */ `
  fragment EditorialSection on EditorialSection {
    id
    title
  }
`)
"#;
    write_project_file(&dir, "components/editorial/fragments.ts", fragment_text);

    let query_text = r#"import { graphql } from 'app/graphql'

export const EditorialHomeDoc = graphql(/* GraphQL */ `
  query EditorialHome {
    editorialHome {
      sections {
        edges {
          node {
            ...EditorialSection
          }
        }
      }
    }
  }
`)
"#;
    let query_uri = write_project_file(&dir, "navigation/home-data.ts", query_text);

    for idx in 0..800 {
        let filler = format!(
            "import {{ graphql }} from 'app/graphql'\nexport const F{idx}Doc = graphql(/* GraphQL */ `fragment F{idx} on EditorialSection {{ id title }}`)\n"
        );
        write_project_file(&dir, &format!("junk/f{idx}.ts"), &filler);
    }

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));
    config = config.with_rules(RulesConfig::default().with_required_fields(required_fields));
    config = config.with_timeouts(TimeoutConfig::default().with_lsp_request_ms(50));

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;
    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    lsp_did_open(&mut service, query_uri.clone(), "typescript", 1, query_text).await;

    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    let diagnostics = match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.items
        }
        _ => panic!("Expected full diagnostic report"),
    };

    assert!(
        diagnostics.iter().all(|diag| {
            diag.code != Some(NumberOrString::String("required_field_missing".to_string()))
        }),
        "Pull diagnostics should wait for workspace fragment context instead of reporting transient required-field errors: {diagnostics:#?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_unchanged_on_bare_epoch_bump() {
    let query_text = "query GetUser { user { id name } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let query_path = base_dir.join("query.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first_result: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;

    let previous_result_id = match first_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => report
            .full_document_diagnostic_report
            .result_id
            .expect("Expected initial result_id"),
        _ => panic!("Expected full diagnostic report on first request"),
    };

    // Bump the workspace epoch directly, without re-caching the document. Diagnostic
    // validity is keyed on the document's own content (version), not the global
    // epoch, so an unchanged document that wasn't re-validated must report Unchanged
    // — a bare epoch bump no longer forces a recompute. (Real changes refresh the
    // document by re-caching it via the affected-document closure.)
    let backend = service.inner();
    backend.workspace_version.store(99, Ordering::SeqCst);

    let second_result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(query_uri),
            identifier: None,
            previous_result_id: Some(previous_result_id.clone()),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match second_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(report)) => {
            assert_eq!(
                report.unchanged_document_diagnostic_report.result_id, previous_result_id,
                "A bare epoch bump must leave an unrevalidated document unchanged"
            );
        }
        other => panic!("Expected an Unchanged report after a bare epoch bump, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_refreshes_when_private_fragment_deletion_revalidates_query() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type User { id: ID! name: String } type Query { me: User }",
        )
        .with_file("pkg_a/package.json", "{}")
        .with_file(
            "pkg_a/public.graphql",
            "fragment UserFields on User @public { id }",
        )
        .with_file("pkg_b/package.json", "{}")
        .with_file(
            "pkg_b/local.graphql",
            "fragment UserFields on User { name }",
        )
        .with_file("pkg_b/query.graphql", "query { me { ...UserFields } }");

    let base_dir = scenario.write_files().unwrap();
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_a/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_b/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, socket) = LspService::new(move |client| {
        graphox::GraphoxLanguageServer::new(Backend::new(client, config))
    });
    let (mut requests, mut responses) = socket.split();
    let (refresh_seen_tx, refresh_seen_rx) = tokio::sync::oneshot::channel();

    let response_task = tokio::spawn(async move {
        let mut refresh_seen_tx = Some(refresh_seen_tx);
        while let Some(request) = requests.next().await {
            if request.method() != "workspace/diagnostic/refresh" {
                continue;
            }

            let request_id = request
                .id()
                .cloned()
                .expect("workspace/diagnostic/refresh should include a request id");

            responses
                .send(Response::from_ok(request_id, serde_json::Value::Null))
                .await
                .expect("failed to send workspace/diagnostic/refresh response");

            if let Some(tx) = refresh_seen_tx.take() {
                tx.send(())
                    .expect("refresh response receiver dropped unexpectedly");
            }
        }
    });

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;
    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let public_path = base_dir.join("pkg_a/public.graphql");
    let local_path = base_dir.join("pkg_b/local.graphql");
    let query_path = base_dir.join("pkg_b/query.graphql");
    let public_uri = graphox::utils::path_to_uri(&public_path).unwrap();
    let local_uri = graphox::utils::path_to_uri(&local_path).unwrap();
    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();

    let public_text = std::fs::read_to_string(&public_path).unwrap();
    let local_text = std::fs::read_to_string(&local_path).unwrap();
    let query_text = std::fs::read_to_string(&query_path).unwrap();

    lsp_did_open(&mut service, public_uri, "graphql", 1, &public_text).await;
    lsp_did_open(&mut service, local_uri.clone(), "graphql", 1, &local_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let first_result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(query_uri.clone()),
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    let previous_result_id = match first_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            assert!(
                report.full_document_diagnostic_report.items.is_empty(),
                "Query should initially resolve the local fragment"
            );
            report
                .full_document_diagnostic_report
                .result_id
                .expect("Expected initial result_id")
        }
        _ => panic!("Expected full diagnostic report on first request"),
    };

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: local_uri,
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: "# local fragment deleted".to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(3), refresh_seen_rx)
        .await
        .expect("timed out waiting for workspace/diagnostic/refresh after fragment deletion")
        .expect("workspace/diagnostic/refresh receiver dropped unexpectedly");

    let refreshed_result: DocumentDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "textDocument/diagnostic",
        &DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier::new(query_uri),
            identifier: None,
            previous_result_id: Some(previous_result_id.clone()),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match refreshed_result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            let refreshed_result_id = report
                .full_document_diagnostic_report
                .result_id
                .expect("Expected refreshed result_id");
            assert_ne!(refreshed_result_id, previous_result_id);
            assert!(
                report.full_document_diagnostic_report.items.is_empty(),
                "Query should fall back to the public fragment after deleting the private one"
            );
        }
        _ => panic!("Expected full diagnostic report after fragment deletion"),
    }

    response_task.abort(); // The task loops forever waiting for refresh requests, so awaiting it would hang the test.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics() {
    let query1_text = "query GetUser { user { id } }";
    let query2_text = "query GetPost { post { title } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User post: Post } type User { id: ID! } type Post { title: String }",
        )
        .with_file("query1.graphql", query1_text)
        .with_file("query2.graphql", query2_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query1_path = base_dir.join("query1.graphql");
    let query2_path = base_dir.join("query2.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    // Initialize with pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    // Open both documents
    let query1_uri = graphox::utils::path_to_uri(&query1_path).unwrap();
    let query2_uri = graphox::utils::path_to_uri(&query2_path).unwrap();

    lsp_did_open(&mut service, query1_uri.clone(), "graphql", 1, query1_text).await;
    lsp_did_open(&mut service, query2_uri.clone(), "graphql", 1, query2_text).await;

    // Request workspace diagnostics
    let workspace_diag_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: vec![],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &workspace_diag_params).await;

    match result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            // Should have diagnostic reports for both documents
            assert!(
                !report.items.is_empty(),
                "Should have workspace diagnostic items"
            );

            // Check that we have reports for our documents
            let mut found_query1 = false;
            let mut found_query2 = false;

            for item in &report.items {
                if let WorkspaceDocumentDiagnosticReport::Full(full_report) = item {
                    if full_report.uri == query1_uri {
                        found_query1 = true;
                    }
                    if full_report.uri == query2_uri {
                        found_query2 = true;
                    }
                }
            }

            assert!(
                found_query1 || found_query2,
                "Should have diagnostics for at least one query"
            );
        }
        _ => panic!("Expected workspace diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_omit_unconfigured_files() {
    let configured_text = "query Configured { user { id } }";
    let ignored_text = "query Ignored { user { invalidField } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("configured.graphql", configured_text)
        .with_file("ignored.graphql", ignored_text);

    let base_dir = scenario.write_files().unwrap();
    let configured_uri = graphox::utils::path_to_uri(base_dir.join("configured.graphql")).unwrap();
    let ignored_uri = graphox::utils::path_to_uri(base_dir.join("ignored.graphql")).unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("configured.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;
    lsp_did_open(
        &mut service,
        ignored_uri.clone(),
        "graphql",
        1,
        ignored_text,
    )
    .await;

    let result: WorkspaceDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "workspace/diagnostic",
        &WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: vec![],
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            let reported_uris: Vec<_> = report
                .items
                .iter()
                .filter_map(|item| match item {
                    WorkspaceDocumentDiagnosticReport::Full(full_report) => {
                        Some(full_report.uri.clone())
                    }
                    WorkspaceDocumentDiagnosticReport::Unchanged(_) => None,
                })
                .collect();

            assert!(
                reported_uris.contains(&configured_uri),
                "Expected configured file to remain in workspace diagnostics"
            );
            assert!(
                !reported_uris.contains(&ignored_uri),
                "Unconfigured files should be omitted from workspace diagnostics"
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_no_mass_refresh_on_bare_epoch_bump() {
    let query1_text = "query GetUser { user { id } }";
    let query2_text = "query GetPost { post { title } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User post: Post } type User { id: ID! } type Post { title: String }",
        )
        .with_file("query1.graphql", query1_text)
        .with_file("query2.graphql", query2_text);

    let base_dir = scenario.write_files().unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let first_result: WorkspaceDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "workspace/diagnostic",
        &WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: vec![],
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    let previous_result_ids = match first_result {
        WorkspaceDiagnosticReportResult::Report(report) => report
            .items
            .into_iter()
            .filter_map(|item| match item {
                WorkspaceDocumentDiagnosticReport::Full(full_report) => full_report
                    .full_document_diagnostic_report
                    .result_id
                    .map(|result_id| PreviousResultId {
                        uri: full_report.uri,
                        value: result_id,
                    }),
                WorkspaceDocumentDiagnosticReport::Unchanged(_) => None,
            })
            .collect::<Vec<_>>(),
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    };

    // Bump the workspace epoch directly, without re-caching any document. Diagnostic
    // validity is keyed on each document's own content (version), not the global
    // epoch, so a bare bump must NOT force a full-workspace revalidation: the poll
    // returns no changed reports. (Real cross-document changes refresh by re-caching
    // the affected documents — see the fragment-deletion tests.)
    let backend = service.inner();
    backend.workspace_version.store(42, Ordering::SeqCst);

    let refreshed_result: WorkspaceDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "workspace/diagnostic",
        &WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match refreshed_result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                report.items.is_empty(),
                "Expected no workspace diagnostics to refresh on a bare epoch bump, got {} items",
                report.items.len()
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_omits_unchanged_items() {
    let query1_text = "query GetUser { user { id } }";
    let query2_text = "query GetPost { post { title } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User post: Post } type User { id: ID! } type Post { title: String }",
        )
        .with_file("query1.graphql", query1_text)
        .with_file("query2.graphql", query2_text);

    let base_dir = scenario.write_files().unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let first_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: vec![],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first_result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &first_params).await;

    let previous_result_ids = match first_result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                !report.items.is_empty(),
                "Expected initial workspace diagnostics"
            );
            report
                .items
                .into_iter()
                .filter_map(|item| match item {
                    WorkspaceDocumentDiagnosticReport::Full(full_report) => full_report
                        .full_document_diagnostic_report
                        .result_id
                        .map(|result_id| PreviousResultId {
                            uri: full_report.uri,
                            value: result_id,
                        }),
                    WorkspaceDocumentDiagnosticReport::Unchanged(_) => None,
                })
                .collect::<Vec<_>>()
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    };

    let second_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let second_result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &second_params).await;

    match second_result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                report.items.is_empty(),
                "Expected unchanged workspace diagnostics to be omitted"
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_returns_empty_while_workspace_loading() {
    let query_text = "query GetUser { user { id } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let backend = service.inner();
    backend.workspace_loaded.store(false, Ordering::SeqCst);

    let workspace_diag_params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: vec![],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: WorkspaceDiagnosticReportResult =
        lsp_request_typed(&mut service, "workspace/diagnostic", &workspace_diag_params).await;

    match result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                report.items.is_empty(),
                "Expected workspace diagnostics to stay empty while the background scan is loading"
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_workspace_diagnostics_omit_schema_files() {
    let schema_text = "\"A described schema root\"\ntype Query { id: ID! }";
    let scenario =
        crate::support::lsp::LspTestScenario::new().with_file("schema.graphqls", schema_text);

    let base_dir = scenario.write_files().unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphqls".to_string()))
                .with_include(GlobPattern::Single("queries/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let result: WorkspaceDiagnosticReportResult = lsp_request_typed(
        &mut service,
        "workspace/diagnostic",
        &WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: vec![],
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await;

    match result {
        WorkspaceDiagnosticReportResult::Report(report) => {
            assert!(
                report.items.is_empty(),
                "Expected schema SDL files to be omitted from workspace diagnostics"
            );
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            panic!("Expected complete workspace diagnostics report")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_return_empty_for_open_schema_file() {
    let schema_text = "\"A described schema root\"\ntype Query { id: ID! }";
    let scenario =
        crate::support::lsp::LspTestScenario::new().with_file("schema.graphqls", schema_text);

    let base_dir = scenario.write_files().unwrap();
    let schema_uri = graphox::utils::path_to_uri(base_dir.join("schema.graphqls")).unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphqls".to_string()))
                .with_include(GlobPattern::Single("queries/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_service(config);

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    let result = lsp_request_diagnostics(&mut service, schema_uri).await;

    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            assert!(
                report.full_document_diagnostic_report.items.is_empty(),
                "Expected open schema files to return no executable-document diagnostics"
            );
        }
        _ => panic!("Expected full diagnostic report"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_fallback_to_push_diagnostics() {
    let query_text = "query GetUser { user { invalidField } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        )
        .with_file("query.graphql", query_text);

    let base_dir = scenario.write_files().unwrap();
    let _schema_path = base_dir.join("schema.graphql");
    let query_path = base_dir.join("query.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = create_lsp_service_with_socket(config);

    // Track push diagnostics
    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_push_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    // Initialize WITHOUT pull diagnostics capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                // No diagnostic capability
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Open document
    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Wait for validation (poll for diagnostics)
    let start = tokio::time::Instant::now();
    loop {
        {
            let push_diags = received_push_diags.lock().unwrap();
            if !push_diags.is_empty() {
                break;
            }
        }
        if start.elapsed() > Duration::from_secs(2) {
            panic!("Timed out waiting for push diagnostics");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Verify push diagnostics WERE sent (fallback behavior)
    let push_diags = received_push_diags.lock().unwrap();
    assert!(
        !push_diags.is_empty(),
        "Should receive push diagnostics when client doesn't support pull"
    );

    let query_diag = push_diags
        .iter()
        .find(|d| d["uri"].as_str().unwrap() == query_uri.as_str())
        .expect("Should have diagnostics for query.graphql");

    assert!(
        !query_diag["diagnostics"].as_array().unwrap().is_empty(),
        "Should have diagnostics for invalid field"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_push_diagnostics_publish_empty_for_unconfigured_file() {
    let configured_text = "query Configured { user { id } }";
    let ignored_text = "query Ignored { user { invalidField } }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .with_file("configured.graphql", configured_text)
        .with_file("ignored.graphql", ignored_text);

    let base_dir = scenario.write_files().unwrap();
    let ignored_path = base_dir.join("ignored.graphql");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("configured.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = create_lsp_service_with_socket(config);

    let received_push_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_push_diags_clone = received_push_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_push_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let _: InitializeResult = lsp_request_typed(&mut service, "initialize", &init_params).await;

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    wait_for_workspace_loaded(&mut service).await;

    let ignored_uri = graphox::utils::path_to_uri(&ignored_path).unwrap();
    lsp_did_open(
        &mut service,
        ignored_uri.clone(),
        "graphql",
        1,
        ignored_text,
    )
    .await;

    let start = tokio::time::Instant::now();
    loop {
        {
            let push_diags = received_push_diags.lock().unwrap();
            if push_diags
                .iter()
                .any(|d| d["uri"].as_str() == Some(ignored_uri.as_str()))
            {
                break;
            }
        }
        if start.elapsed() > Duration::from_secs(2) {
            panic!("Timed out waiting for push diagnostics for unconfigured file");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let push_diags = received_push_diags.lock().unwrap();
    let ignored_reports: Vec<_> = push_diags
        .iter()
        .filter(|d| d["uri"].as_str() == Some(ignored_uri.as_str()))
        .collect();

    assert!(
        !ignored_reports.is_empty(),
        "Expected an empty push diagnostic publication for the unconfigured file"
    );
    assert!(
        ignored_reports.iter().all(|report| report["diagnostics"]
            .as_array()
            .is_some_and(|items| items.is_empty())),
        "Unconfigured files should only publish empty diagnostics: {:?}",
        ignored_reports
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pull_diagnostics_refresh_after_config_reload_restores_fragment_context() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String! }",
        )
        .with_file("query.graphql", "query GetUser { user { ...UserFields } }")
        .with_file(
            "fragments.graphql",
            "fragment UserFields on User { id name }",
        )
        .with_file(
            "graphox.yaml",
            r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
    codegen: false
rules:
  required_fields:
    id: true
"#,
        );

    let base_dir = scenario.write_files().unwrap();
    let query_path = base_dir.join("query.graphql");
    let config_path = base_dir.join("graphox.yaml");

    let config = Config::load_from_dir(&base_dir).unwrap().unwrap();
    let (mut service, mut messages) = create_lsp_service_with_socket(config);
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(4);

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("window/logMessage") {
                let params_json = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let params: LogMessageParams = serde_json::from_value(params_json).unwrap();
                if params.message.starts_with("Workspace scan complete") {
                    let _ = scan_done_tx.send(()).await;
                }
            }
        }
    });

    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result: InitializeResult =
        lsp_request_typed(&mut service, "initialize", &init_params).await;
    assert!(result.capabilities.diagnostic_provider.is_some());

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_millis(5000), scan_done_rx.recv())
        .await
        .expect("Initial workspace scan did not complete in time")
        .expect("scan_done_rx closed before initial scan completed");

    let query_uri = graphox::utils::path_to_uri(&query_path).unwrap();
    let query_text = std::fs::read_to_string(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let diag_params = DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(query_uri.clone()),
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let before_reload: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;
    let before_items = match before_reload {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.items
        }
        _ => panic!("Expected full diagnostic report before reload"),
    };
    assert!(
        before_items.iter().all(|diag| {
            diag.code != Some(NumberOrString::String("required_field_missing".to_string()))
        }),
        "Fragment selections should satisfy required fields before config reload: {before_items:#?}"
    );

    std::fs::write(
        &config_path,
        r#"
projects:
  - schema: schema.graphql
    include: "*.graphql"
    codegen: false
rules:
  required_fields:
    id: true
# reload
"#,
    )
    .unwrap();

    let changes = vec![FileEvent {
        uri: graphox::utils::path_to_uri(&config_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }];

    // Drain any buffered scan_done messages before triggering the change
    while scan_done_rx.try_recv().is_ok() {}

    service
        .call(
            tower_lsp_server::jsonrpc::Request::build("workspace/didChangeWatchedFiles")
                .params(serde_json::to_value(DidChangeWatchedFilesParams { changes }).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_millis(5000), scan_done_rx.recv())
        .await
        .expect("Reload workspace scan did not complete in time")
        .expect("scan_done_rx closed before reload scan completed");

    let after_reload: DocumentDiagnosticReportResult =
        lsp_request_typed(&mut service, "textDocument/diagnostic", &diag_params).await;
    let after_items = match after_reload {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            report.full_document_diagnostic_report.items
        }
        _ => panic!("Expected full diagnostic report after reload"),
    };
    assert!(
        after_items.iter().all(|diag| {
            diag.code != Some(NumberOrString::String("required_field_missing".to_string()))
        }),
        "Pull diagnostics should be refreshed after reload so cross-file fragment fields remain visible: {after_items:#?}"
    );
}
