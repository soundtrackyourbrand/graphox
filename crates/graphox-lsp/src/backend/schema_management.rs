//! Schema management and reloading
//!
//! This module handles schema lifecycle operations including
//! loading, reloading, validation, and cache management.

use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use dashmap::DashMap;
use graphox_core::config::{Config, SchemaSource};
use std::path::Path;
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Reloads schemas that contain the changed file
pub async fn reload_schema(
    changed_path: &str,
    config: &Config,
    schemas: &Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    validated_schemas: &Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    client: &Client,
    supports_progress: bool,
) -> Vec<String> {
    let mut sources_to_reload = Vec::new();

    // Check if any project schemas contain this file
    for project in config.projects() {
        if schema_contains_file(project.schema(), changed_path, config.base_dir()) {
            sources_to_reload.push(project.schema().clone());
        }
    }

    // Check schema_types
    for st in config.schema_types() {
        if schema_contains_file(st.schema(), changed_path, config.base_dir()) {
            sources_to_reload.push(st.schema().clone());
        }
    }

    if sources_to_reload.is_empty() {
        return Vec::new();
    }

    // Create progress reporter for schema reload
    let progress = super::progress::ProgressReporter::new(
        client.clone(),
        format!("Reloading {} schema(s)", sources_to_reload.len()),
        supports_progress,
    )
    .await;

    let mut reloaded_keys = Vec::new();
    let total = sources_to_reload.len();

    for (idx, source) in sources_to_reload.into_iter().enumerate() {
        let key = source.as_key();

        progress
            .report(
                format!("Loading schema {}/{}...", idx + 1, total),
                Some(((idx + 1) * 100 / total) as u32),
            )
            .await;

        let new_schema = graphox_core::schema::load_schema_arc(config.base_dir(), &source);

        match new_schema {
            Some(new_schema) => {
                if let Ok(valid) =
                    <apollo_compiler::Schema as Clone>::clone(&*new_schema).validate()
                {
                    validated_schemas.insert(key.clone(), Arc::new(valid));
                } else {
                    client
                        .log_message(
                            MessageType::WARNING,
                            format!("Schema validation failed for {}: schema is invalid but will still be used", key),
                        )
                        .await;
                }
                schemas.insert(key.clone(), new_schema.clone());
                client
                    .log_message(
                        MessageType::INFO,
                        format!("Schema set {} successfully reloaded!", key),
                    )
                    .await;
                reloaded_keys.push(key);
            }
            None => {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!("Failed to load schema {}: check that schema files exist and are valid GraphQL", key),
                    )
                    .await;
            }
        }
    }

    progress
        .end(Some(format!("Reloaded {} schema(s)", reloaded_keys.len())))
        .await;

    reloaded_keys
}

/// Clears all schema caches and reloads from config
pub async fn clear_cache(
    config: &Config,
    schemas: &Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    validated_schemas: &Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    client: &Client,
) {
    schemas.clear();
    validated_schemas.clear();

    // Reload project schemas from config
    for project in config.projects() {
        let key = project.schema().as_key();
        if !schemas.contains_key(&key) {
            match graphox_core::schema::load_schema_arc(config.base_dir(), project.schema()) {
                Some(schema) => {
                    if let Ok(valid) =
                        <apollo_compiler::Schema as Clone>::clone(&*schema).validate()
                    {
                        validated_schemas.insert(key.clone(), Arc::new(valid));
                    } else {
                        client
                            .log_message(
                                MessageType::WARNING,
                                format!("Schema validation failed for {}: schema is invalid but will still be used", key),
                            )
                            .await;
                    }
                    schemas.insert(key, schema);
                }
                None => {
                    client
                        .log_message(
                            MessageType::ERROR,
                            format!("Failed to load schema {}: check that schema files exist and are valid GraphQL", key),
                        )
                        .await;
                }
            }
        }
    }

    client
        .log_message(MessageType::INFO, "Cache cleared and schemas reloaded!")
        .await;
}

/// Gets URIs affected by a schema reload
pub fn get_uris_affected_by_schema<F>(
    schema_key: &str,
    config: &Config,
    get_all_uris: F,
) -> Vec<tower_lsp::lsp_types::Url>
where
    F: Fn() -> Vec<tower_lsp::lsp_types::Url>,
{
    let all_uris = get_all_uris();
    all_uris
        .into_iter()
        .filter(|uri| {
            if let Ok(doc_path) = uri.to_file_path() {
                config
                    .get_schema_for_path(&doc_path)
                    .is_some_and(|p| p.as_str() == schema_key)
            } else {
                false
            }
        })
        .collect()
}

/// Checks if a schema source contains a specific file
fn schema_contains_file(source: &SchemaSource, changed_path: &str, base_dir: &Path) -> bool {
    let changed_path_obj = Path::new(changed_path);
    source.files().iter().any(|f| {
        let abs = base_dir.join(f);
        if graphox_core::utils::paths_match(Some(&abs), Some(changed_path_obj)) {
            return true;
        }

        if let Ok(canonical_abs) = abs.canonicalize()
            && graphox_core::utils::paths_match(Some(&canonical_abs), Some(changed_path_obj))
        {
            return true;
        }

        false
    })
}
