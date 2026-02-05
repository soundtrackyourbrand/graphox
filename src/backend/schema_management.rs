//! Schema management and reloading
//!
//! This module handles schema lifecycle operations including
//! loading, reloading, validation, and cache management.

use crate::config::{Config, SchemaSource};
use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use dashmap::DashMap;
use std::path::Path;
use std::sync::Arc;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;

/// Reloads schemas that contain the changed file
pub async fn reload_schema(
    changed_path: &str,
    config: &Config,
    schemas: &Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    validated_schemas: &Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    client: &Client,
) -> Vec<String> {
    let mut sources_to_reload = Vec::new();
    
    // Check if any project schemas contain this file
    for project in &config.projects {
        if schema_contains_file(&project.schema, changed_path, &config.base_dir) {
            sources_to_reload.push(project.schema.clone());
        }
    }
    
    // Check schema_types
    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            if schema_contains_file(&st.schema, changed_path, &config.base_dir) {
                sources_to_reload.push(st.schema.clone());
            }
        }
    }

    let mut reloaded_keys = Vec::new();
    
    for source in sources_to_reload {
        let key = source.as_key();
        let new_schema = crate::schema::load_schema_arc(&config.base_dir, &source);

        if let Some(new_schema) = new_schema {
            if let Ok(valid) = <apollo_compiler::Schema as Clone>::clone(&*new_schema).validate() {
                validated_schemas.insert(key.clone(), Arc::new(valid));
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
    }
    
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
    for project in &config.projects {
        let key = project.schema.as_key();
        if !schemas.contains_key(&key)
            && let Some(schema) = crate::schema::load_schema_arc(&config.base_dir, &project.schema) {
                if let Ok(valid) = <apollo_compiler::Schema as Clone>::clone(&*schema).validate() {
                    validated_schemas.insert(key.clone(), Arc::new(valid));
                }
                schemas.insert(key, schema);
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
    source.files().iter().any(|f| {
        let abs = base_dir.join(f);
        abs.to_string_lossy() == changed_path
            || abs
                .canonicalize()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
                == Some(changed_path.to_string())
    })
}
