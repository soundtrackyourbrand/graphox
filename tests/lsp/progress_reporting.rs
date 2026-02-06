use futures_util::StreamExt;
use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
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
async fn test_progress_on_workspace_scan() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    // Create multiple files to trigger progress
    for i in 0..20 {
        let query_path = base_dir.join(format!("query{}.graphql", i));
        fs::write(
            &query_path,
            format!("query GetUser{} {{ user {{ id name }} }}", i),
        )
        .unwrap();
    }

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
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
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "$/progress"
                && let Some(params) = msg.params()
            {
                progress_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    // Initialize with progress capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // Wait for workspace scan to complete and progress notifications to arrive
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let notifications = progress_notifications.lock().unwrap();
        let has_end = notifications.iter().any(|n| {
            n.get("value")
                .and_then(|v| v.get("kind"))
                .and_then(|k| k.as_str())
                == Some("end")
        });
        if has_end {
            break;
        }
    }

    // Verify progress notifications were sent
    let notifications = progress_notifications.lock().unwrap();
    assert!(
        !notifications.is_empty(),
        "Should receive progress notifications for workspace scan"
    );

    // Check for begin, report, and end notifications
    let has_begin = notifications.iter().any(|n| {
        n.get("value")
            .and_then(|v| v.get("kind"))
            .and_then(|k| k.as_str())
            == Some("begin")
    });

    let _has_report = notifications.iter().any(|n| {
        n.get("value")
            .and_then(|v| v.get("kind"))
            .and_then(|k| k.as_str())
            == Some("report")
    });

    let has_end = notifications.iter().any(|n| {
        n.get("value")
            .and_then(|v| v.get("kind"))
            .and_then(|k| k.as_str())
            == Some("end")
    });

    assert!(has_begin, "Should have begin progress notification");
    assert!(has_end, "Should have end progress notification");
    // Report may or may not appear depending on timing
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_no_progress_without_capability() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    // Create multiple files
    for i in 0..20 {
        let query_path = base_dir.join(format!("query{}.graphql", i));
        fs::write(
            &query_path,
            format!("query GetUser{} {{ user {{ id }} }}", i),
        )
        .unwrap();
    }

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
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
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "$/progress"
                && let Some(params) = msg.params()
            {
                progress_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    // Initialize WITHOUT progress capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            // No work_done_progress capability
            ..Default::default()
        },
        ..Default::default()
    };

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // Wait for workspace scan to complete
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let notifications = progress_notifications.lock().unwrap();
        if !notifications.is_empty() {
            break;
        }
    }

    // Verify NO progress notifications were sent
    let notifications = progress_notifications.lock().unwrap();
    assert!(
        notifications.is_empty(),
        "Should NOT receive progress notifications when client doesn't support it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_progress_on_codegen() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query GetMe { me }").unwrap();

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
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "$/progress"
                && let Some(params) = msg.params()
            {
                progress_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    // Initialize with progress capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // Wait for workspace scan
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Clear previous notifications
    progress_notifications.lock().unwrap().clear();

    // Trigger codegen
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
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify codegen progress notifications
    let notifications = progress_notifications.lock().unwrap();

    // Check if any progress notification mentions codegen/generating
    let has_codegen_progress = notifications.iter().any(|n| {
        if let Some(value) = n.get("value")
            && let Some(message) = value.get("message").and_then(|m| m.as_str())
        {
            return message.to_lowercase().contains("generat")
                || message.to_lowercase().contains("typescript")
                || message.to_lowercase().contains("types");
        }
        false
    });

    assert!(
        has_codegen_progress,
        "Should receive progress notifications for codegen"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_progress_messages_contain_percentage() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();

    // Create multiple files to ensure progress reporting
    for i in 0..15 {
        let query_path = base_dir.join(format!("query{}.graphql", i));
        fs::write(
            &query_path,
            format!("query GetUser{} {{ user {{ id }} }}", i),
        )
        .unwrap();
    }

    let config = Config {
        output_dir: None,
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("*.graphql".to_string()),
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
        lsp_codegen_throttle_ms: None,
        timeouts: None,
    };

    let (mut service, mut messages) = LspService::new(|client| Backend::new(client, config));

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.method() == "$/progress"
                && let Some(params) = msg.params()
            {
                progress_clone.lock().unwrap().push(params.clone());
            }
        }
    });

    // Initialize with progress capability
    let init_params = InitializeParams {
        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // Wait for workspace scan and progress with percentage
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let notifications = progress_notifications.lock().unwrap();
        let has_percentage = notifications.iter().any(|n| {
            n.get("value")
                .and_then(|v| v.get("percentage"))
                .and_then(|p| p.as_u64())
                .is_some()
        });
        if has_percentage {
            break;
        }
    }

    // Check for percentage in progress notifications
    let notifications = progress_notifications.lock().unwrap();

    let has_percentage = notifications.iter().any(|n| {
        n.get("value")
            .and_then(|v| v.get("percentage"))
            .and_then(|p| p.as_u64())
            .is_some()
    });

    assert!(
        has_percentage,
        "Progress notifications should include percentage values"
    );
}
