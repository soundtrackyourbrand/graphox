//! File system change handling
//!
//! This module handles LSP didChangeWatchedFiles notifications,
//! processing file creations, changes, and deletions by updating
//! internal indices and metadata.

use ahash::AHashSet;
use graphox_core::DocumentState;
use graphox_core::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDependentsMap, MetadataMap,
    OperationNamesMap,
};
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Result of processing a file change
pub struct FileChangeResult {
    pub uris_to_validate: Vec<Url>,
    pub should_reload_schema: bool,
    pub schema_path: Option<String>,
    pub should_run_codegen: bool,
    pub should_reload_config: bool,
}

/// Parameters for file change processing
pub struct FileChangeParams<'a> {
    pub client: &'a Client,
    pub config: &'a graphox_core::Config,
    pub documents: &'a DocumentsMap,
    pub metadata: &'a MetadataMap,
    pub fragment_dependents: &'a FragmentDependentsMap,
    pub fragment_definitions: &'a FragmentDefinitionsMap,
    pub operation_names: &'a OperationNamesMap,
    pub gitignore: &'a ignore::gitignore::Gitignore,
    pub diagnostic_cache: &'a DiagnosticCacheMap,
    pub position_encoding: PositionEncodingKind,
}

/// Processes a file creation or change
pub async fn process_file_created_or_changed(
    change_uri: Url,
    params: &FileChangeParams<'_>,
    normalize_uri: impl Fn(Url) -> Url,
) -> Option<FileChangeResult> {
    let uri = normalize_uri(change_uri);
    let path = uri.to_file_path().ok()?;
    let path_str = path.to_string_lossy().to_string();

    // Check if this is the config file
    if is_config_file(&path, params.config) {
        return Some(FileChangeResult {
            uris_to_validate: vec![],
            should_reload_schema: false,
            schema_path: None,
            should_run_codegen: false,
            should_reload_config: true,
        });
    }

    // Skip ignored files
    if !graphox_core::utils::is_relevant_file(&path)
        || graphox_core::utils::is_path_ignored(&path, params.gitignore)
    {
        return None;
    }

    if params.config.is_output_file(&path) {
        return None;
    }

    let is_schema = is_schema_file(&path, params.config);

    // Read file content
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            super::error_logging::log_warning(
                params.client,
                "File change handler",
                format!("Failed to read file {}: {}", path.display(), e),
            )
            .await;
            return None;
        }
    };

    if graphox_core::utils::has_generated_header(&content) {
        return None;
    }

    let new_doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &content,
        params.position_encoding.clone(),
    );

    if !is_schema && new_doc.get_graphql_trees().is_empty() {
        return None;
    }

    let mut affected_fragment_names = AHashSet::default();
    let mut affected_spread_names = AHashSet::default();
    let mut affected_operation_names = AHashSet::default();

    let old_fragments: Option<Arc<[Arc<str>]>> = params.metadata.get(&uri).map(|m| {
        m.fragments
            .iter()
            .map(|f| f.name.clone())
            .collect::<Arc<[_]>>()
    });
    let old_spreads: Option<Arc<[Arc<str>]>> = params
        .metadata
        .get(&uri)
        .map(|m| m.fragment_spreads.clone());

    let new_fragment_defs = new_doc.fragments.clone();
    let new_fragment_names: Arc<[Arc<str>]> =
        new_fragment_defs.iter().map(|f| f.name.clone()).collect();
    let new_spreads: Arc<[Arc<str>]> = new_doc.fragment_spreads.clone();

    // Track changes to fragment definitions
    if let Some(old) = &old_fragments {
        for name in old.iter() {
            if !new_fragment_names.contains(name) {
                affected_fragment_names.insert(name.clone());
            }
        }
    }
    for name in new_fragment_names.iter() {
        if old_fragments.as_ref().is_none_or(|old| !old.contains(name)) {
            affected_fragment_names.insert(name.clone());
        }
    }

    // Track changes to fragment spreads
    if let Some(old) = &old_spreads {
        for name in old.iter() {
            if !new_spreads.contains(name) {
                affected_spread_names.insert(name.clone());
            }
        }
    }
    for name in new_spreads.iter() {
        if old_spreads.as_ref().is_none_or(|old| !old.contains(name)) {
            affected_spread_names.insert(name.clone());
        }
    }

    // Update metadata
    let metadata = Arc::new(graphox_core::types::DocumentMetadata {
        fragments: new_fragment_defs,
        fragment_spreads: new_spreads.clone(),
        package_root: new_doc.package_root.clone(),
        operations: new_doc.operations.clone(),
        version: new_doc.version,
    });
    params.metadata.insert(uri.clone(), metadata);

    super::fragment_manager::update_fragment_definitions(
        params.fragment_definitions,
        &uri,
        old_fragments,
        new_fragment_names,
    );
    super::fragment_manager::update_fragment_dependents(
        params.fragment_dependents,
        &uri,
        old_spreads,
        new_spreads,
    );

    // Update operation name index
    if let Some(schema_key) = params.config.get_schema_for_path(&path) {
        let project_key = params
            .config
            .get_project_for_path(&path)
            .map(|p| p.include().as_key())
            .unwrap_or_else(|| schema_key.clone());
        let project_key_arc: Arc<str> = project_key.into();

        // Remove old operations for this URI
        for mut entry in params.operation_names.iter_mut() {
            let op_name = entry.key().clone();
            let mut removed = false;
            entry.value_mut().retain(|(_, op_uri)| {
                if op_uri == &uri {
                    removed = true;
                    false
                } else {
                    true
                }
            });
            if removed {
                affected_operation_names.insert(op_name);
            }
        }
        params.operation_names.retain(|_, v| !v.is_empty());

        // Add new operations
        for op in new_doc.operations() {
            if let Some(name) = &op.name {
                affected_operation_names.insert(name.clone());
                params
                    .operation_names
                    .entry(name.clone())
                    .or_default()
                    .push((project_key_arc.clone(), uri.clone()));
            }
        }
    }

    // Enable codegen for watched file changes if it contains GraphQL
    let should_run_codegen = !new_doc.get_graphql_trees().is_empty();

    // Update documents map (only if we want to keep it in memory - for now we do)
    params.documents.insert(uri.clone(), Arc::new(new_doc));

    let uris_to_validate = super::validation::get_affected_uris(
        uri,
        affected_fragment_names,
        affected_spread_names,
        affected_operation_names,
        params.documents,
        params.fragment_dependents,
        params.fragment_definitions,
        params.operation_names,
    );

    Some(FileChangeResult {
        uris_to_validate,
        should_reload_schema: is_schema,
        schema_path: if is_schema { Some(path_str) } else { None },
        should_run_codegen,
        should_reload_config: false,
    })
}

/// Processes a file deletion
pub fn process_file_deleted(
    change_uri: Url,
    params: &FileChangeParams<'_>,
    normalize_uri: impl Fn(Url) -> Url,
) -> Option<FileChangeResult> {
    let uri = normalize_uri(change_uri);
    let path = uri.to_file_path().ok()?;

    // Check if this is the config file
    if is_config_file(&path, params.config) {
        return Some(FileChangeResult {
            uris_to_validate: vec![],
            should_reload_schema: false,
            schema_path: None,
            should_run_codegen: false,
            should_reload_config: true,
        });
    }

    if !graphox_core::utils::is_relevant_file(&path)
        || graphox_core::utils::is_path_ignored(&path, params.gitignore)
    {
        return None;
    }

    let mut affected_fragment_names = AHashSet::default();
    let mut affected_spread_names = AHashSet::default();
    let mut affected_operation_names = AHashSet::default();

    let old_fragments: Option<Arc<[Arc<str>]>> = params.metadata.get(&uri).map(|m| {
        m.fragments
            .iter()
            .map(|f| f.name.clone())
            .collect::<Arc<[_]>>()
    });
    let old_spreads: Option<Arc<[Arc<str>]>> = params
        .metadata
        .get(&uri)
        .map(|m| m.fragment_spreads.clone());

    if let Some(old) = &old_fragments {
        for name in old.iter() {
            affected_fragment_names.insert(name.clone());
        }
    }
    if let Some(old) = &old_spreads {
        for name in old.iter() {
            affected_spread_names.insert(name.clone());
        }
    }

    // Clean up metadata
    params.documents.remove(&uri);
    params.metadata.remove(&uri);
    params.diagnostic_cache.remove(&uri);

    super::fragment_manager::update_fragment_definitions(
        params.fragment_definitions,
        &uri,
        old_fragments,
        Arc::from([]),
    );
    super::fragment_manager::update_fragment_dependents(
        params.fragment_dependents,
        &uri,
        old_spreads,
        Arc::from([]),
    );

    // Remove operations from index
    for mut entry in params.operation_names.iter_mut() {
        let op_name = entry.key().clone();
        let mut removed = false;
        entry.value_mut().retain(|(_, op_uri)| {
            if op_uri == &uri {
                removed = true;
                false
            } else {
                true
            }
        });
        if removed {
            affected_operation_names.insert(op_name);
        }
    }
    params.operation_names.retain(|_, v| !v.is_empty());

    let uris_to_validate = super::validation::get_affected_uris(
        uri,
        affected_fragment_names,
        affected_spread_names,
        affected_operation_names,
        params.documents,
        params.fragment_dependents,
        params.fragment_definitions,
        params.operation_names,
    );

    Some(FileChangeResult {
        uris_to_validate,
        should_reload_schema: false,
        schema_path: None,
        should_run_codegen: false,
        should_reload_config: false,
    })
}

/// Checks if a path is the config file
fn is_config_file(path: &std::path::Path, config: &graphox_core::Config) -> bool {
    let config_path = config.base_dir().join("graphox.yaml");
    let config_yml_path = config.base_dir().join("graphox.yml");
    graphox_core::utils::paths_match(Some(path), Some(&config_path))
        || graphox_core::utils::paths_match(Some(path), Some(&config_yml_path))
}

/// Checks if a path is a schema file
fn is_schema_file(path: &std::path::Path, config: &graphox_core::Config) -> bool {
    config.projects().iter().any(|p| {
        p.schema().files().iter().any(|f| {
            let abs_schema = config.base_dir().join(f);
            graphox_core::utils::paths_match(Some(path), Some(&abs_schema))
        })
    }) || config.schema_types().iter().any(|st| {
        st.schema().files().iter().any(|f| {
            let abs_schema = config.base_dir().join(f);
            graphox_core::utils::paths_match(Some(path), Some(&abs_schema))
        })
    })
}
