use crate::support::{self, create_initialized_lsp_service, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use graphox::{Config, config::CodegenConfig, config::GlobPattern, config::ProjectConfig, config::SchemaSource};
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, sleep};
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_command_clear_cache() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema.graphql", "type Query { me: String }")
        .with_file("query.graphql", "query { me }");

    let base_dir = scenario.write_files().unwrap();

    let config = Config::new_test(
        base_dir.clone(),
        vec![ProjectConfig::default()
            .with_schema(SchemaSource::Single("schema.graphql".to_string()))
            .with_include(GlobPattern::Single("query.graphql".to_string()))
            .with_codegen(CodegenConfig::disabled())],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                received_diags_clone.lock().unwrap().push(params);
            }
        }
    });

    lsp_initialize_sequence(&mut service).await;

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me }";
    let query_uri = Url::from_file_path(&query_path).unwrap();
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Initial diagnostics (should be empty)
    sleep(Duration::from_millis(10)).await;
    {
        let diags = received_diags.lock().unwrap();
        assert!(
            diags.last().unwrap()["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    // Change schema on disk WITHOUT notifying the LSP
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { someoneElse: String }").unwrap();

    // Trigger clear cache
    let params = ExecuteCommandParams {
        command: "graphql.clearCache".to_string(),
        arguments: vec![],
        ..Default::default()
    };
    service
        .call(
            Request::build("workspace/executeCommand")
                .params(serde_json::to_value(params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap();

    // Wait for re-validation diagnostics
    for _ in 0..50 {
        sleep(Duration::from_millis(10)).await;
        let diags = received_diags.lock().unwrap();
        if diags.len() > 1 {
            // We have new diagnostics after cache clear
            break;
        }
    }
    {
        let diags = received_diags.lock().unwrap();
        let last = diags.last().unwrap();
        let d_list = last["diagnostics"].as_array().unwrap();
        assert!(
            !d_list.is_empty(),
            "Should have diagnostics after clearCache reloads schema"
        );
        assert!(d_list[0]["message"].as_str().unwrap().contains("me"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_command_run_codegen() {
    let query_text = "query GetMe { me }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema.graphql", "type Query { me: String }")
        .with_file("query.graphql", query_text)
        .with_file("generated/.keep", "");

    let base_dir = scenario.write_files().unwrap();
    let output_dir = "generated";

    let config = Config::new_test(
        base_dir.clone(),
        vec![ProjectConfig::default()
            .with_schema(SchemaSource::Single("schema.graphql".to_string()))
            .with_include(GlobPattern::Single("query.graphql".to_string()))
            .with_output_dir(output_dir.to_string())],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Trigger run codegen
    let params = ExecuteCommandParams {
        command: "graphql.runCodegen".to_string(),
        arguments: vec![],
        ..Default::default()
    };
    service
        .call(
            Request::build("workspace/executeCommand")
                .params(serde_json::to_value(params).unwrap())
                .id(1)
                .finish(),
        )
        .await
        .unwrap();

    // Verify files were generated
    let codegen_file = base_dir.join(output_dir).join("query.codegen.ts");
    let entrypoint_file = base_dir.join(output_dir).join("graphql.ts");

    // Wait for codegen to complete
    for _ in 0..100 {
        sleep(Duration::from_millis(10)).await;
        if codegen_file.exists() && entrypoint_file.exists() {
            break;
        }
    }

    assert!(codegen_file.exists(), "Codegen file should be generated");
    assert!(
        entrypoint_file.exists(),
        "Entrypoint file should be generated"
    );

    let content = fs::read_to_string(codegen_file).unwrap();
    assert!(
        content.contains("GetMeQuery"),
        "Should contain generated type"
    );
}
