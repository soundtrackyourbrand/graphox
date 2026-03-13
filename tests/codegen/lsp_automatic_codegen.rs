use crate::support;
use futures_util::StreamExt;
use graphox::{
    Config, config::CodegenConfig, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use graphox_cli::run_codegen as run_cli_codegen;
use std::collections::BTreeMap;
use std::fs;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

fn snapshot_generated_tree(root: &std::path::Path) -> BTreeMap<String, String> {
    fn walk(
        current: &std::path::Path,
        root: &std::path::Path,
        files: &mut BTreeMap<String, String>,
    ) {
        let entries = match fs::read_dir(current) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, files);
                continue;
            }

            let rel = path
                .strip_prefix(root)
                .expect("failed to strip prefix for test file path")
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("failed to read test file: {}", path.display()));
            files.insert(rel, content);
        }
    }

    let mut files = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut files);
    }
    files
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetMe { me { id } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string())),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let query_uri = Url::from_file_path(&query_path).unwrap();
    let gen_path = base_dir.join("query.codegen.ts");

    // 1. Initial codegen (triggered by didOpen if we wanted, but let's test didChange)
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

    // Wait for codegen
    assert!(
        support::wait_for_file_async(&gen_path, Duration::from_millis(2000), Some("GetMe")).await
    );
    let content = fs::read_to_string(&gen_path).unwrap();
    // Use a more specific check to avoid matching schema types or comments if any
    assert!(
        !content.contains("name: string"),
        "Generated content should not contain 'name' field: {}",
        content
    );

    // 2. didChange should trigger codegen
    let query_text_new = "query GetMyProfile { me { id name } }";
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: query_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: query_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for automatic codegen
    assert!(
        support::wait_for_file_async(&gen_path, Duration::from_millis(2000), Some("GetMyProfile"))
            .await,
        "didChange should trigger codegen when automatic codegen is enabled"
    );
    let changed_content = fs::read_to_string(&gen_path).unwrap();
    assert!(
        changed_content.contains("name: string | null"),
        "Generated content should contain 'name' field after didChange: {}",
        changed_content
    );

    // 3. didSave should also trigger codegen (idempotent if no changes, but here we can just verify it works)
    service
        .call(
            Request::build("textDocument/didSave")
                .params(
                    serde_json::to_value(DidSaveTextDocumentParams {
                        text_document: TextDocumentIdentifier {
                            uri: query_uri.clone(),
                        },
                        text: None,
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let updated =
        support::wait_for_file_async(&gen_path, Duration::from_millis(2000), Some("GetMyProfile"))
            .await;
    assert!(updated, "Codegen was not updated after didSave");

    // 4. Test didChangeWatchedFiles triggers codegen
    // Close the document first to ensure LSP uses disk content for didChangeWatchedFiles
    service
        .call(
            Request::build("textDocument/didClose")
                .params(
                    serde_json::to_value(DidCloseTextDocumentParams {
                        text_document: TextDocumentIdentifier {
                            uri: query_uri.clone(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    fs::remove_file(&gen_path).unwrap();
    let query_text_watched = "query GetMe { me { name } }";
    fs::write(&query_path, query_text_watched).unwrap();

    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(
                    serde_json::to_value(DidChangeWatchedFilesParams {
                        changes: vec![FileEvent {
                            uri: query_uri.clone(),
                            typ: FileChangeType::CHANGED,
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for codegen
    assert!(
        support::wait_for_file_async(
            &gen_path,
            Duration::from_millis(1000),
            Some("name: string | null")
        )
        .await
    );
    let content = fs::read_to_string(&gen_path).unwrap();
    assert!(content.contains("name: string | null"));
    assert!(!content.contains("id: string"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_disabled() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User users: [User!]! }",
    )
    .unwrap();

    // Create two query files - one for enabled project, one for disabled
    let enabled_query_path = base_dir.join("enabled.graphql");
    let enabled_query_text = "query GetMe { me { id } }";
    fs::write(&enabled_query_path, enabled_query_text).unwrap();

    let disabled_query_path = base_dir.join("disabled.graphql");
    let disabled_query_text = "query GetUsers { users { id name } }";
    fs::write(&disabled_query_path, disabled_query_text).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("enabled.graphql".to_string()))
                .with_codegen(CodegenConfig::enabled()), // Explicitly enabled
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("disabled.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()), // Disabled
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let enabled_uri = Url::from_file_path(&enabled_query_path).unwrap();
    let disabled_uri = Url::from_file_path(&disabled_query_path).unwrap();
    let enabled_gen_path = base_dir.join("enabled.codegen.ts");
    let disabled_gen_path = base_dir.join("disabled.codegen.ts");

    // Open enabled query file - should trigger codegen
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: enabled_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: enabled_query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for enabled codegen
    assert!(
        support::wait_for_file_async(&enabled_gen_path, Duration::from_millis(2000), None).await
    );
    let enabled_content = fs::read_to_string(&enabled_gen_path).unwrap();
    assert!(enabled_content.contains("GetMeQuery"));

    // Open disabled query file - should NOT trigger codegen
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: disabled_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: disabled_query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait a bit to ensure no codegen happens
    sleep(Duration::from_millis(10)).await;

    // Verify disabled project did NOT generate files
    assert!(
        !disabled_gen_path.exists(),
        "Should not generate files for disabled project, but found: {}",
        disabled_gen_path.display()
    );

    // Test didChange + didSave on disabled project - should still not generate
    let disabled_query_text_new = "query GetUsers { users { id name } }";
    fs::write(&disabled_query_path, disabled_query_text_new).unwrap();
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: disabled_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: disabled_query_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("textDocument/didSave")
                .params(
                    serde_json::to_value(DidSaveTextDocumentParams {
                        text_document: TextDocumentIdentifier {
                            uri: disabled_uri.clone(),
                        },
                        text: None,
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait again to ensure no codegen happens
    sleep(Duration::from_millis(150)).await;
    assert!(
        !disabled_gen_path.exists(),
        "Should still not generate files after didSave for disabled project"
    );

    // Verify enabled project triggers codegen on didChange
    let enabled_query_text_new = "query GetMe { me { id name } }";
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: enabled_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: enabled_query_text_new.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for automatic codegen
    assert!(
        support::wait_for_file_async(
            &enabled_gen_path,
            Duration::from_millis(2000),
            Some("name: string | null")
        )
        .await,
        "didChange should trigger enabled project codegen when automatic codegen is enabled"
    );

    // didSave should also trigger codegen
    service
        .call(
            Request::build("textDocument/didSave")
                .params(
                    serde_json::to_value(DidSaveTextDocumentParams {
                        text_document: TextDocumentIdentifier {
                            uri: enabled_uri.clone(),
                        },
                        text: None,
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for updated codegen on enabled project
    assert!(
        support::wait_for_file_async(
            &enabled_gen_path,
            Duration::from_millis(1000),
            Some("name: string | null")
        )
        .await,
        "Enabled project codegen was not updated after didSave"
    );
    let enabled_updated_content = fs::read_to_string(&enabled_gen_path).unwrap();
    assert!(
        enabled_updated_content.contains("name: string | null"),
        "Enabled project codegen did not contain expected field after didSave"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_disabled_project_before_enabled_keeps_entrypoint_alignment() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let disabled_path = base_dir.join("disabled.ts");
    fs::write(
        &disabled_path,
        r#"
        import { graphql } from "./disabled_gen/graphql";
        export const DisabledDoc = graphql(/* GraphQL */ `
          query DisabledDoc {
            me { id }
          }
        `);
        "#,
    )
    .unwrap();

    let enabled_path = base_dir.join("enabled.ts");
    fs::write(
        &enabled_path,
        r#"
        import { graphql } from "./enabled_gen/graphql";
        export const EnabledDoc = graphql(/* GraphQL */ `
          query EnabledDoc {
            me {
              id
              markerName: name
            }
          }
        `);
        "#,
    )
    .unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("disabled.ts".to_string()))
                .with_output_dir("disabled_gen".to_string())
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("enabled.ts".to_string()))
                .with_output_dir("enabled_gen".to_string()),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let enabled_uri = Url::from_file_path(&enabled_path).unwrap();
    service
        .call(
            Request::build("textDocument/didSave")
                .params(
                    serde_json::to_value(DidSaveTextDocumentParams {
                        text_document: TextDocumentIdentifier { uri: enabled_uri },
                        text: None,
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let enabled_entrypoint = base_dir.join("enabled_gen/graphql.ts");
    assert!(
        support::wait_for_file_async(&enabled_entrypoint, Duration::from_millis(1000), None).await
    );

    let enabled_content = fs::read_to_string(&enabled_entrypoint).unwrap();
    assert!(
        enabled_content.contains("markerName: name"),
        "Enabled project entrypoint should include the enabled operation source"
    );

    let disabled_entrypoint = base_dir.join("disabled_gen/graphql.ts");
    if disabled_entrypoint.exists() {
        let disabled_content = fs::read_to_string(&disabled_entrypoint).unwrap();
        assert!(
            !disabled_content.contains("markerName: name"),
            "Disabled project entrypoint must not receive enabled project fragments/operations"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_didsave_uses_disk_state_for_ts_host() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let host_path = base_dir.join("query.ts");
    fs::write(
        &host_path,
        r#"
        import { graphql } from "./gen/graphql";

        export const GetMe = graphql(/* GraphQL */ `
          query GetMe {
            me { id }
          }
        `);
        "#,
    )
    .unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.ts".to_string()))
                .with_output_dir("gen".to_string()),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let host_uri = Url::from_file_path(&host_path).unwrap();
    service
        .call(
            Request::build("textDocument/didSave")
                .params(
                    serde_json::to_value(DidSaveTextDocumentParams {
                        text_document: TextDocumentIdentifier { uri: host_uri },
                        text: None,
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let gen_path = base_dir.join("gen/query.codegen.ts");
    assert!(support::wait_for_file_async(&gen_path, Duration::from_millis(2000), None).await);
    let content = fs::read_to_string(&gen_path).unwrap();
    assert!(content.contains("GetMeQuery"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_didsave_syncs_in_memory_when_disk_stale() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.ts");
    let query_text = r#"
      import { graphql } from "./gen/graphql";
      export const GetMe = graphql(/* GraphQL */ `
        query GetMe {
          me { id }
        }
      `);
    "#;
    fs::write(&query_path, query_text).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.ts".to_string()))
                .with_output_dir("gen".to_string()),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "typescript".to_string(),
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

    let changed_text = r#"
      import { graphql } from "./gen/graphql";
      export const GetMe = graphql(/* GraphQL */ `
        query GetMe {
          me { id name }
        }
      `);
    "#;

    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: query_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: changed_text.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    service
        .call(
            Request::build("textDocument/didSave")
                .params(
                    serde_json::to_value(DidSaveTextDocumentParams {
                        text_document: TextDocumentIdentifier {
                            uri: query_uri.clone(),
                        },
                        text: None,
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let gen_path = base_dir.join("gen/query.codegen.ts");
    assert!(support::wait_for_file_async(&gen_path, Duration::from_millis(2000), None).await);

    let mut content = fs::read_to_string(&gen_path).unwrap();
    for _ in 0..40 {
        if content.contains("name: string | null") {
            break;
        }
        sleep(Duration::from_millis(20)).await;
        content = fs::read_to_string(&gen_path).unwrap();
    }
    assert!(
        content.contains("name: string | null"),
        "didSave should use latest in-memory text even if disk was stale"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_no_loop_on_output_files() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query GetMe { me { id } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_output_dir("gen".to_string()),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let gen_dir = base_dir.join("gen");
    let gen_path = gen_dir.join("query.codegen.ts");
    let manifest_path = gen_dir.join("manifest.json");
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(
                    serde_json::to_value(DidChangeWatchedFilesParams {
                        changes: vec![FileEvent {
                            uri: query_uri.clone(),
                            typ: FileChangeType::CHANGED,
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Wait for the last file written by run_codegen to ensure the run is fully complete
    assert!(support::wait_for_file_async(&manifest_path, Duration::from_millis(2000), None).await);

    assert!(
        gen_path.exists(),
        "Codegen file should exist after triggering codegen"
    );

    let output_file_path = gen_dir.join("output.ts");
    fs::write(&output_file_path, "export const foo = 'bar';").unwrap();

    let output_uri = Url::from_file_path(&output_file_path).unwrap();

    let initial_file_count = std::fs::read_dir(&gen_dir)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().is_file())
        .count();

    for _ in 0..10 {
        fs::write(
            &output_file_path,
            format!(
                "export const foo = 'bar{}';",
                std::time::Instant::now().elapsed().as_millis()
            ),
        )
        .unwrap();

        service
            .call(
                Request::build("workspace/didChangeWatchedFiles")
                    .params(
                        serde_json::to_value(DidChangeWatchedFilesParams {
                            changes: vec![FileEvent {
                                uri: output_uri.clone(),
                                typ: FileChangeType::CHANGED,
                            }],
                        })
                        .unwrap(),
                    )
                    .finish(),
            )
            .await
            .unwrap();

        sleep(Duration::from_millis(50)).await;
    }

    sleep(Duration::from_millis(100)).await;

    let final_file_count = std::fs::read_dir(&gen_dir)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().is_file())
        .count();

    if initial_file_count != final_file_count {
        let files: Vec<_> = std::fs::read_dir(&gen_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        println!("Files in gen_dir: {:?}", files);
    }

    assert_eq!(
        initial_file_count, final_file_count,
        "Codegen loop detected: output file changes triggered additional codegen runs"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_ignores_non_graphql_host_edits() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    fs::write(
        base_dir.join("schema.graphql"),
        "type User { id: ID! } type Query { me: User }",
    )
    .unwrap();
    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query GetMe { me { id } }").unwrap();
    let plain_path = base_dir.join("plain.ts");
    fs::write(&plain_path, "export const value = 1;\n").unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("query.graphql".to_string()))
                .with_output_dir("gen".to_string()),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let query_uri = Url::from_file_path(&query_path).unwrap();
    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(
                    serde_json::to_value(DidChangeWatchedFilesParams {
                        changes: vec![FileEvent {
                            uri: query_uri,
                            typ: FileChangeType::CHANGED,
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let gen_path = base_dir.join("gen/query.codegen.ts");
    assert!(support::wait_for_file_async(&gen_path, Duration::from_millis(2000), None).await);
    fs::write(
        &gen_path,
        "// touched by formatter\nexport const untouched = true;\n",
    )
    .unwrap();

    let plain_uri = Url::from_file_path(&plain_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: plain_uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: "export const value = 1;\n".to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    fs::write(&plain_path, "export const value = 2;\n").unwrap();
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: plain_uri,
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: "export const value = 2;\n".to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(2000)).await;
    let content = fs::read_to_string(&gen_path).unwrap();
    assert!(
        content.starts_with("// touched by formatter"),
        "Non-GraphQL host edit should not trigger codegen"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_regenerates_nested_ts_host_for_directory_include() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    fs::write(
        base_dir.join("schema.graphql"),
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let host_path = base_dir.join("apps/mobile/app/navigation/main/home/home-data.ts");
    fs::create_dir_all(host_path.parent().unwrap()).unwrap();

    let initial_text = r#"
      import { graphql } from "app/graphql";

      export const HomeDataDoc = graphql(/* GraphQL */ `
        query HomeData {
          me { id }
        }
      `);
    "#;
    fs::write(&host_path, initial_text).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("apps/mobile/app".to_string()))
                .with_output_dir("apps/mobile/app/graphql".to_string())
                .with_codegen(
                    CodegenConfig::default()
                        .with_generate_ast_for_fragments(true)
                        .with_fragment_suffix("Fragment".to_string())
                        .with_fragment_document_suffix("Doc".to_string())
                        .with_re_exports(true),
                ),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    let mut service = setup_lsp_with_scan_complete(config).await;

    let host_uri = Url::from_file_path(&host_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: host_uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: initial_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let gen_path =
        base_dir.join("apps/mobile/app/graphql/navigation/main/home/home-data.codegen.ts");
    assert!(
        support::wait_for_file_async(
            &gen_path,
            Duration::from_millis(2000),
            Some("HomeDataQuery")
        )
        .await,
        "didOpen should generate codegen for nested TS hosts covered by a directory include"
    );

    let changed_text = r#"
      import { graphql } from "app/graphql";

      export const HomeDataDoc = graphql(/* GraphQL */ `
        query HomeDataUpdated {
          me { id name }
        }
      `);
    "#;

    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: host_uri,
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: changed_text.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    assert!(
        support::wait_for_file_async(
            &gen_path,
            Duration::from_millis(2000),
            Some("HomeDataUpdatedQuery"),
        )
        .await,
        "didChange should regenerate the nested TS host codegen file"
    );

    let generated = fs::read_to_string(&gen_path).unwrap();
    assert!(
        generated.contains("name: string | null"),
        "Regenerated codegen should include the updated field selection"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_lsp_automatic_codegen_matches_cli_bundle_for_directory_include_project() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    fs::write(
        base_dir.join("schema.graphql"),
        r#"
        type Query {
          me: User
        }

        type User {
          id: ID!
          name: String
          display: Display
        }

        type Display {
          title: String
        }
        "#,
    )
    .unwrap();

    let playback_path = base_dir.join("apps/mobile/app/lib/playback/graphql.ts");
    fs::create_dir_all(playback_path.parent().unwrap()).unwrap();
    fs::write(
        &playback_path,
        r#"
        import type { ResultOf } from '@graphql-typed-document-node/core';
        import { graphql } from 'app/graphql';

        const PlaybackDisplayFragmentDoc = graphql(/* GraphQL */ `
          fragment PlaybackDisplay on Display {
            title
          }
        `);
        export type PlaybackDisplayFragment = ResultOf<typeof PlaybackDisplayFragmentDoc>;
        "#,
    )
    .unwrap();

    let home_path = base_dir.join("apps/mobile/app/navigation/main/home/home-data.ts");
    fs::create_dir_all(home_path.parent().unwrap()).unwrap();
    let home_text = r#"
      import { graphql } from "app/graphql";

      export const HomeDataDoc = graphql(/* GraphQL */ `
        query HomeData {
          me {
            id
          }
        }
      `);
    "#;
    fs::write(&home_path, home_text).unwrap();

    let config = Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("apps/mobile/app".to_string()))
                .with_output_dir("apps/mobile/app/graphql".to_string())
                .with_codegen(
                    CodegenConfig::default()
                        .with_generate_ast_for_fragments(true)
                        .with_fragment_suffix("Fragment".to_string())
                        .with_fragment_document_suffix("Doc".to_string())
                        .with_re_exports(true),
                ),
        ],
    )
    .with_lsp_automatic_codegen(true)
    .with_lsp_codegen_throttle_ms(50)
    .with_enable_schema_cache(true);

    assert!(
        config
            .get_codegen_config(Some(&config.projects()[0]))
            .generate_ast_for_fragments(),
        "Fixture config should enable fragment AST generation"
    );

    run_cli_codegen(config.clone(), false, false, false).await;

    let output_dir = base_dir.join("apps/mobile/app/graphql");
    let cli_snapshot = snapshot_generated_tree(&output_dir);
    assert!(
        cli_snapshot.contains_key("lib/playback/graphql.codegen.ts"),
        "CLI snapshot should contain generated playback fragment module"
    );
    assert!(
        cli_snapshot["lib/playback/graphql.codegen.ts"].contains("PlaybackDisplayFragmentDoc"),
        "CLI snapshot should export the playback fragment document. content={}",
        cli_snapshot["lib/playback/graphql.codegen.ts"]
    );

    fs::remove_dir_all(&output_dir).unwrap();

    let mut service = setup_lsp_with_scan_complete(config).await;

    let playback_uri = Url::from_file_path(&playback_path).unwrap();
    {
        // This manipulation is intentional for testing the cold-cache scenario (i.e., to simulate files
        // absent from the live cache). It couples the test to internal state (backend.metadata, backend.documents)
        // so that we can verify the engine correctly re-discovers files from disk when they are missing from
        // the in-memory cache but still present on disk and within the project's include patterns.
        let backend = service.inner();
        backend.metadata.remove(&playback_uri);
        backend.documents.remove(&playback_uri);
    }

    let home_uri = Url::from_file_path(&home_path).unwrap();
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: home_uri,
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: home_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let playback_codegen = output_dir.join("lib/playback/graphql.codegen.ts");
    assert!(
        support::wait_for_file_async(
            &playback_codegen,
            Duration::from_millis(2000),
            Some("PlaybackDisplayFragmentDoc"),
        )
        .await,
        "LSP auto-codegen should regenerate fragment-only project files even when they were absent from the live metadata cache. exists={} content={}",
        playback_codegen.exists(),
        if playback_codegen.exists() {
            fs::read_to_string(&playback_codegen).unwrap()
        } else {
            "<missing>".to_string()
        }
    );
    assert!(
        service.inner().documents.get(&playback_uri).is_none(),
        "LSP codegen should not populate backend.documents for unopened files from disk during codegen runs"
    );
    assert!(
        support::wait_for_file_async(
            &output_dir.join("manifest.json"),
            Duration::from_millis(2000),
            Some("PlaybackDisplayFragmentDoc"),
        )
        .await,
        "LSP auto-codegen should finish writing the manifest before comparing bundle snapshots"
    );

    let lsp_snapshot = snapshot_generated_tree(&output_dir);
    assert_eq!(
        lsp_snapshot, cli_snapshot,
        "LSP auto-codegen output should match CLI codegen for directory-include projects"
    );
}

async fn setup_lsp_with_scan_complete(config: Config) -> LspService<support::LspBackend> {
    let (mut service, mut messages) = support::create_lsp_service_with_socket(config);
    let (scan_done_tx, mut scan_done_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        while let Some(msg) = messages.next().await {
            if msg.get("method").and_then(|m| m.as_str()) == Some("window/logMessage") {
                let params_json = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                if let Ok(params) = serde_json::from_value::<LogMessageParams>(params_json)
                    && params.message.starts_with("Workspace scan complete")
                {
                    let _ = scan_done_tx.send(()).await;
                }
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

    let _ = tokio::time::timeout(Duration::from_millis(10000), scan_done_rx.recv())
        .await
        .expect("Scan did not complete in time");

    service
}
