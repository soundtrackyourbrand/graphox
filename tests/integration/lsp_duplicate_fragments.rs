use crate::support::{self, lsp_did_open, lsp_initialize_sequence};
use futures_util::StreamExt;
use graphql_rust::{
    config::{GlobPattern, ProjectConfig, SchemaSource},
    Config,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::time::Duration;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(100)]
async fn test_lsp_duplicate_fragments_same_project_via_config() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir_all(&pkg_a).unwrap();
    let frag_a_path = pkg_a.join("frag_a.graphql");
    fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();

    let pkg_b = base_dir.join("pkg_b");
    fs::create_dir_all(&pkg_b).unwrap();
    let frag_b_path = pkg_b.join("frag_b.graphql");
    fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

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
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
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

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let uri_b = Url::from_file_path(&frag_b_path).unwrap();

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
    assert!(d_a
        .iter()
        .any(|d| d.message.contains("Duplicate fragment name: 'DuplicateFrag'")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(100)]
async fn test_lsp_private_duplicates_different_projects_no_error() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let pkg_a = base_dir.join("pkg_a");
    fs::create_dir_all(&pkg_a).unwrap();
    let frag_a_path = pkg_a.join("frag_a.graphql");
    fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();

    let pkg_b = base_dir.join("pkg_b");
    fs::create_dir_all(&pkg_b).unwrap();
    let frag_b_path = pkg_b.join("frag_b.graphql");
    fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let config = Config {
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_a/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("pkg_b/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
        ],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let received_diags = Arc::new(Mutex::new(
        std::collections::HashMap::<Url, Vec<Diagnostic>>::new(),
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

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let uri_b = Url::from_file_path(&frag_b_path).unwrap();

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