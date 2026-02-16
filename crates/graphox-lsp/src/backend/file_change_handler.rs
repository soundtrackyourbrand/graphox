//! File change handler module
//!
//! This module handles processing file system changes for both schema files
//! and GraphQL document files, updating indices and triggering validation.

use ahash::AHashSet;
use graphox_core::config::Config;
use graphox_core::document::{DocumentLanguage, DocumentState};
use graphox_core::types::{
    DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap, FragmentDependentsMap,
    FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use graphox_core::utils::{is_path_ignored, is_relevant_file, path_starts_with};
use std::path::Path;
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

fn is_output_file(path: &Path, config: &Config) -> bool {
    for project in config.projects() {
        if let Some(output_dir) = project.output_dir() {
            let abs_output = config.base_dir().join(output_dir);
            if path_starts_with(path, &abs_output) {
                return true;
            }
        }
    }
    false
}

/// Parameters for file change processing
pub struct FileChangeParams<'a> {
    pub client: &'a Client,
    pub config: &'a Config,
    pub documents: &'a DocumentsMap,
    pub fragment_defs: &'a FragmentDefsMap,
    pub fragment_spreads: &'a FragmentSpreadsMap,
    pub package_roots: &'a PackageRootsMap,
    pub fragment_dependents: &'a FragmentDependentsMap,
    pub fragment_definitions: &'a FragmentDefinitionsMap,
    pub operation_names: &'a OperationNamesMap,
    pub gitignore: &'a ignore::gitignore::Gitignore,
    pub position_encoding: PositionEncodingKind,
}

/// Result of processing a file change
pub struct FileChangeResult {
    pub uris_to_validate: Vec<Url>,
    pub should_reload_schema: bool,
    pub schema_path: Option<String>,
    pub should_run_codegen: bool,
    pub should_reload_config: bool,
}

/// Processes a single file creation or modification
pub async fn process_file_created_or_changed(
    change_uri: Url,
    params: &FileChangeParams<'_>,
    normalize_uri: impl Fn(Url) -> Url,
) -> Option<FileChangeResult> {
    let path = match change_uri.to_file_path() {
        Ok(p) => p,
        Err(_) => {
            super::error_logging::log_warning(
                params.client,
                "File change handler",
                format!("Invalid file path in URI: {}", change_uri),
            )
            .await;
            return None;
        }
    };
    let path_str = path.to_string_lossy().to_string();

    // Ignore changes to files in output directories to prevent infinite codegen loops
    if is_output_file(&path, params.config) {
        return None;
    }

    // Check if this is a config file
    if is_config_file(&path, params.config) {
        return Some(FileChangeResult {
            uris_to_validate: vec![],
            should_reload_schema: false,
            schema_path: None,
            should_run_codegen: false,
            should_reload_config: true,
        });
    }

    // Check if this is a schema file
    if is_schema_file(&path_str, params.config) {
        return Some(FileChangeResult {
            uris_to_validate: vec![],
            should_reload_schema: true,
            schema_path: Some(path_str),
            should_run_codegen: false,
            should_reload_config: false,
        });
    }

    // Handle GraphQL document files
    if !is_relevant_file(&path) || is_path_ignored(&path, params.gitignore) {
        return None;
    }

    let uri = normalize_uri(change_uri);
    let mut affected_fragment_names = AHashSet::default();
    let mut affected_spread_names = AHashSet::default();

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

    let _language = DocumentLanguage::from_uri(&uri);
    let new_doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &content,
        params.position_encoding.clone(),
    );

    let old_fragments: Option<Vec<Arc<str>>> = params
        .fragment_defs
        .get(&uri)
        .map(|f| f.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    let old_spreads: Option<Vec<Arc<str>>> =
        params.fragment_spreads.get(&uri).map(|s| s.value().clone());

    let new_fragment_defs = new_doc.fragments().to_vec();
    let new_fragment_names: Vec<Arc<str>> =
        new_fragment_defs.iter().map(|f| f.name.clone()).collect();
    let new_spreads: Vec<Arc<str>> = new_doc.fragment_spreads.clone();

    // Track changes to fragment definitions
    if let Some(old) = &old_fragments {
        for name in old {
            if !new_fragment_names.contains(name) {
                affected_fragment_names.insert(name.clone());
            }
        }
    }
    for name in &new_fragment_names {
        if old_fragments.as_ref().is_none_or(|old| !old.contains(name)) {
            affected_fragment_names.insert(name.clone());
        }
    }

    // Track changes to fragment spreads
    if let Some(old) = &old_spreads {
        for name in old {
            if !new_spreads.contains(name) {
                affected_spread_names.insert(name.clone());
            }
        }
    }
    for name in &new_spreads {
        if old_spreads.as_ref().is_none_or(|old| !old.contains(name)) {
            affected_spread_names.insert(name.clone());
        }
    }

    // Update metadata
    params.fragment_defs.insert(uri.clone(), new_fragment_defs);
    params
        .fragment_spreads
        .insert(uri.clone(), new_spreads.clone());
    params
        .package_roots
        .insert(uri.clone(), new_doc.package_root.clone());

    update_definition_indices(
        params.fragment_definitions,
        &uri,
        old_fragments,
        new_fragment_names,
    );
    update_dependency_indices(params.fragment_dependents, &uri, old_spreads, new_spreads);

    // Update operation name index
    if let Some(schema_key) = params.config.get_schema_for_path(&path) {
        // Remove old operations for this URI
        let mut operations_to_update: Vec<Arc<str>> = Vec::new();
        for mut entry in params.operation_names.iter_mut() {
            let op_name = entry.key().clone();
            entry.value_mut().retain(|(_, op_uri)| op_uri != &uri);
            if entry.value().is_empty() {
                operations_to_update.push(op_name);
            }
        }
        // Clean up empty entries
        for op_name in operations_to_update {
            params.operation_names.remove(&op_name);
        }

        // Add new operations
        let schema_key_arc: Arc<str> = schema_key.into();
        for op in new_doc.operations() {
            if let Some(name) = &op.name {
                params
                    .operation_names
                    .entry(name.clone())
                    .or_default()
                    .push((schema_key_arc.clone(), uri.clone()));
            }
        }
    }

    // Update documents map if we have it
    if params.documents.contains_key(&uri) {
        params
            .documents
            .insert(uri.clone(), Arc::new(new_doc.clone()));
    }

    let uris_to_validate = super::validation::get_affected_uris(
        uri,
        affected_fragment_names,
        affected_spread_names,
        params.documents,
        params.fragment_dependents,
        params.fragment_definitions,
    );

    let should_run_codegen =
        params.config.lsp_automatic_codegen() && !new_doc.get_graphql_trees().is_empty();

    Some(FileChangeResult {
        uris_to_validate,
        should_reload_schema: false,
        schema_path: None,
        should_run_codegen,
        should_reload_config: false,
    })
}

/// Processes a file deletion
pub fn process_file_deleted(
    change_uri: Url,
    params: &FileChangeParams,
    normalize_uri: impl Fn(Url) -> Url,
) -> Option<FileChangeResult> {
    let uri = normalize_uri(change_uri);
    let mut affected_fragment_names = AHashSet::default();
    let mut affected_spread_names = AHashSet::default();

    let old_fragments = params
        .fragment_defs
        .get(&uri)
        .map(|f| f.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    let old_spreads = params.fragment_spreads.get(&uri).map(|s| s.clone());

    if let Some(old) = &old_fragments {
        for name in old {
            affected_fragment_names.insert(name.clone());
        }
    }
    if let Some(old) = &old_spreads {
        for name in old {
            affected_spread_names.insert(name.clone());
        }
    }

    // Clean up metadata
    params.documents.remove(&uri);
    params.fragment_defs.remove(&uri);
    params.fragment_spreads.remove(&uri);
    params.package_roots.remove(&uri);
    update_definition_indices(params.fragment_definitions, &uri, old_fragments, vec![]);
    update_dependency_indices(params.fragment_dependents, &uri, old_spreads, vec![]);

    // Remove operations from index
    let mut operations_to_clean: Vec<Arc<str>> = Vec::new();
    for mut entry in params.operation_names.iter_mut() {
        let op_name = entry.key().clone();
        entry.value_mut().retain(|(_, op_uri)| op_uri != &uri);
        if entry.value().is_empty() {
            operations_to_clean.push(op_name);
        }
    }
    for op_name in operations_to_clean {
        params.operation_names.remove(op_name.as_ref());
    }

    let uris_to_validate = super::validation::get_affected_uris(
        uri,
        affected_fragment_names,
        affected_spread_names,
        params.documents,
        params.fragment_dependents,
        params.fragment_definitions,
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
fn is_config_file(path: &std::path::Path, config: &Config) -> bool {
    let config_yaml = config.base_dir().join("graphox.yaml");
    let config_yml = config.base_dir().join("graphox.yml");

    path == config_yaml
        || path == config_yml
        || path.canonicalize().ok() == config_yaml.canonicalize().ok()
        || path.canonicalize().ok() == config_yml.canonicalize().ok()
}

/// Checks if a path is a schema file
fn is_schema_file(path_str: &str, config: &Config) -> bool {
    for project in config.projects() {
        if project.schema().files().iter().any(|f| {
            let abs = config.base_dir().join(f);
            abs.to_string_lossy() == path_str
                || abs
                    .canonicalize()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    == Some(path_str.to_string())
        }) {
            return true;
        }
    }

    for st in config.schema_types() {
        if st.schema().files().iter().any(|f| {
            let abs = config.base_dir().join(f);
            abs.to_string_lossy() == path_str
                || abs
                    .canonicalize()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    == Some(path_str.to_string())
        }) {
            return true;
        }
    }

    false
}

/// Updates fragment definition indices
fn update_definition_indices(
    fragment_definitions: &graphox_core::types::FragmentDefinitionsMap,
    uri: &Url,
    old_fragments: Option<Vec<Arc<str>>>,
    new_fragments: Vec<Arc<str>>,
) {
    super::fragment_manager::update_fragment_definitions(
        fragment_definitions,
        uri,
        old_fragments,
        new_fragments,
    );
}

/// Updates fragment dependency indices
fn update_dependency_indices(
    fragment_dependents: &graphox_core::types::FragmentDependentsMap,
    uri: &Url,
    old_spreads: Option<Vec<Arc<str>>>,
    new_spreads: Vec<Arc<str>>,
) {
    super::fragment_manager::update_fragment_dependents(
        fragment_dependents,
        uri,
        old_spreads,
        new_spreads,
    );
}
