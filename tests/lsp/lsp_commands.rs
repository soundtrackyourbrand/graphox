use futures_util::StreamExt;
use graphql_rust::{
    Backend, Config, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_command_clear_cache() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    let received_diags = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received_diags_clone = received_diags.clone();
    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "textDocument/publishDiagnostics" {
                let params = msg.params().unwrap();
                received_diags_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Initial diagnostics (should be empty)
    tokio::time::sleep(Duration::from_millis(200)).await;
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
    tokio::time::sleep(Duration::from_millis(500)).await;
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
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetMe { me }";
    fs::write(&query_path, query_text).unwrap();

    let output_dir = "generated";
    fs::create_dir(base_dir.join(output_dir)).unwrap();

    let config = Config {
        output_dir: Some(output_dir.to_string()),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None, // This test needs codegen enabled
        }],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
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
        .unwrap();

    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

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

    // Wait for codegen to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify files were generated
    let codegen_file = base_dir.join(output_dir).join("query.codegen.ts");
    let entrypoint_file = base_dir.join(output_dir).join("graphql.ts");

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
