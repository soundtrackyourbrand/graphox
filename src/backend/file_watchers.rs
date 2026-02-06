//! File watcher registration for LSP
//!
//! This module handles registering file system watchers with the LSP client
//! to monitor changes to schema files and workspace GraphQL files.

use crate::config::Config;
use fnv::FnvHashSet;
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Registers file watchers for schema files, workspace files, and config file
///
/// This function extracts the file watcher registration logic from Backend::initialized().
/// It runs in a separate tokio task to avoid blocking if the client doesn't respond immediately.
pub fn register_file_watchers(client: Client, config: &Config) {
    let mut watchers = Vec::new();
    let mut schema_files = FnvHashSet::default();

    // Watch the config file itself (graphql.yaml or graphql.yml)
    let config_yaml = config.base_dir.join("graphql.yaml");
    let config_yml = config.base_dir.join("graphql.yml");

    if config_yaml.exists() {
        watchers.push(FileSystemWatcher {
            glob_pattern: GlobPattern::String(config_yaml.to_string_lossy().to_string()),
            kind: Some(WatchKind::all()),
        });
    } else if config_yml.exists() {
        watchers.push(FileSystemWatcher {
            glob_pattern: GlobPattern::String(config_yml.to_string_lossy().to_string()),
            kind: Some(WatchKind::all()),
        });
    }

    // Collect all schema files from projects
    for project in &config.projects {
        for file in project.schema.files() {
            schema_files.insert(file);
        }
    }

    // Collect schema files from schema_types
    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            for file in st.schema.files() {
                schema_files.insert(file);
            }
        }
    }

    // Create watchers for schema files
    for file in schema_files {
        watchers.push(FileSystemWatcher {
            glob_pattern: GlobPattern::String(file),
            kind: Some(WatchKind::all()),
        });
    }

    // If configured, watch all relevant files in the workspace
    if config.watch_all_files() {
        watchers.push(FileSystemWatcher {
            glob_pattern: GlobPattern::String(
                "**/*.{graphql,gql,ts,tsx,mts,cts,js,jsx,mjs,cjs}".to_string(),
            ),
            kind: Some(WatchKind::all()),
        });
    }

    // Register the watchers with the client
    let registration = Registration {
        id: "watch-files".to_string(),
        method: "workspace/didChangeWatchedFiles".to_string(),
        register_options: Some(
            serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers }).unwrap(),
        ),
    };

    tokio::spawn(async move {
        if let Err(e) = client.register_capability(vec![registration]).await {
            client
                .log_message(
                    MessageType::ERROR,
                    format!("Failed to register schema watcher: {}", e),
                )
                .await;
        }
    });
}
