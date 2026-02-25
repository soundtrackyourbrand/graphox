use crate::support::{self};
use futures_util::StreamExt;
use graphox::{
    Config,
    config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource},
};
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_progress_on_workspace_scan() {
    // Build files via LspTestScenario to control layout
    let scenario = crate::support::lsp::LspTestScenario::new();
    let scenario = (0..20).fold(
        scenario.with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! name: String }",
        ),
        |s, i| {
            s.with_file(
                &format!("query{}.graphql", i),
                &format!("query GetUser{} {{ user {{ id name }} }}", i),
            )
        },
    );

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
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                progress_clone.lock().unwrap().push(params);
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

    let has_end = notifications.iter().any(|n| {
        n.get("value")
            .and_then(|v| v.get("kind"))
            .and_then(|k| k.as_str())
            == Some("end")
    });

    assert!(has_begin, "Should have begin progress notification");
    assert!(has_end, "Should have end progress notification");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_no_progress_without_capability() {
    let scenario = crate::support::lsp::LspTestScenario::new();
    let scenario = (0..20).fold(
        scenario.with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        ),
        |s, i| {
            s.with_file(
                &format!("query{}.graphql", i),
                &format!("query GetUser{} {{ user {{ id }} }}", i),
            )
        },
    );
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
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                progress_clone.lock().unwrap().push(params);
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
    let query_text = "query GetMe { me }";
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("schema.graphql", "type Query { me: String }")
        .with_file("query.graphql", query_text)
        .with_file("generated/.keep", "");

    let base_dir = scenario.write_files().unwrap();
    let output_dir = "generated";

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_output_dir(output_dir.to_string()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                progress_clone.lock().unwrap().push(params);
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
        command: "graphox.runCodegen".to_string(),
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
    let scenario = crate::support::lsp::LspTestScenario::new();
    let scenario = (0..15).fold(
        scenario.with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        ),
        |s, i| {
            s.with_file(
                &format!("query{}.graphql", i),
                &format!("query GetUser{} {{ user {{ id }} }}", i),
            )
        },
    );
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
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                progress_clone.lock().unwrap().push(params);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(10000)]
async fn test_progress_messages_during_validation() {
    let scenario = crate::support::lsp::LspTestScenario::new();
    // Create enough files to ensure validation takes some time and progress is reported
    let scenario = (0..30).fold(
        scenario.with_file(
            "schema.graphql",
            "type Query { user: User } type User { id: ID! }",
        ),
        |s, i| {
            s.with_file(
                &format!("query{}.graphql", i),
                &format!("query GetUser{} {{ user {{ id }} }}", i),
            )
        },
    );
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
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);

    // Track progress notifications
    let progress_notifications = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let progress_clone = progress_notifications.clone();

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                progress_clone.lock().unwrap().push(params);
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

    // Wait for workspace scan to complete (up to 5 seconds)
    let mut validation_percentages = Vec::new();
    for _ in 0..500 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let notifications = progress_notifications.lock().unwrap();

        for n in notifications.iter() {
            if let Some(value) = n.get("value") {
                let message = value.get("message").and_then(|m| m.as_str());
                let percentage = value.get("percentage").and_then(|p| p.as_u64());

                if let Some(msg) = message
                    && msg.contains("Validating")
                    && let Some(p) = percentage
                {
                    validation_percentages.push(p);
                }
            }
        }

        let has_end = notifications.iter().any(|n| {
            n.get("value")
                .and_then(|v| v.get("kind"))
                .and_then(|k| k.as_str())
                == Some("end")
        });

        if has_end {
            if !validation_percentages.is_empty() {
                break;
            }
            // If we have end but no validation progress yet, wait a bit more or log
            eprintln!(
                "[DEBUG TEST] got end but no validation progress. total notifications: {}",
                notifications.len()
            );
            for (i, n) in notifications.iter().enumerate() {
                eprintln!("[DEBUG TEST] notification {}: {:?}", i, n);
            }
        }
    }

    assert!(
        !validation_percentages.is_empty(),
        "Should receive progress notifications specifically for validation phase containing 'Validating'"
    );

    // Check that we got at least one percentage >= 70
    assert!(
        validation_percentages.iter().any(|&p| p >= 70),
        "Should have at least one validation progress report starting at >= 70%"
    );
}
