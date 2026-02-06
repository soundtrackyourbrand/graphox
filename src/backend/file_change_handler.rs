//! File change handler module
//!
//! This module handles processing file system changes for both schema files
//! and GraphQL document files, updating indices and triggering validation.

use crate::config::Config;
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{is_path_ignored, is_relevant_file};
use dashmap::DashMap;
use fnv::FnvHashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;

/// Parameters for file change processing
pub struct FileChangeParams<'a> {
    pub client: &'a Client,
    pub config: &'a Config,
    pub documents: &'a Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    pub fragment_defs: &'a Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    pub fragment_spreads: &'a Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    pub package_roots: &'a Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    pub fragment_dependents: &'a Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub fragment_definitions: &'a Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub gitignore: &'a ignore::gitignore::Gitignore,
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
pub fn process_file_created_or_changed(
    change_uri: Url,
    params: &FileChangeParams,
    normalize_uri: impl Fn(Url) -> Url,
) -> Option<FileChangeResult> {
    let path = change_uri.to_file_path().ok()?;
    let path_str = path.to_string_lossy().to_string();

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
    let mut affected_fragment_names = FnvHashSet::default();
    let mut affected_spread_names = FnvHashSet::default();

    let content = std::fs::read_to_string(&path).ok()?;
    let language = DocumentLanguage::from_uri(&uri);
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language.get_parser_language()).ok()?;
    let new_doc = DocumentState::new(uri.clone(), &content, parser);

    let old_fragments = params
        .fragment_defs
        .get(&uri)
        .map(|f| f.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    let old_spreads = params.fragment_spreads.get(&uri).map(|s| s.clone());

    let new_fragment_defs = new_doc.fragments().to_vec();
    let new_fragment_names: Vec<_> = new_fragment_defs.iter().map(|f| f.name.clone()).collect();
    let new_spreads = new_doc.fragment_spreads.clone();

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

    // Update documents map if we have it
    if params.documents.contains_key(&uri) {
        params.documents.insert(uri.clone(), Arc::new(new_doc));
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
        should_run_codegen: params.config.lsp_automatic_codegen(),
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
    let mut affected_fragment_names = FnvHashSet::default();
    let mut affected_spread_names = FnvHashSet::default();

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
    let config_yaml = config.base_dir.join("graphql.yaml");
    let config_yml = config.base_dir.join("graphql.yml");

    path == config_yaml
        || path == config_yml
        || path.canonicalize().ok() == config_yaml.canonicalize().ok()
        || path.canonicalize().ok() == config_yml.canonicalize().ok()
}

/// Checks if a path is a schema file
fn is_schema_file(path_str: &str, config: &Config) -> bool {
    for project in &config.projects {
        if project.schema.files().iter().any(|f| {
            let abs = config.base_dir.join(f);
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

    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            if st.schema.files().iter().any(|f| {
                let abs = config.base_dir.join(f);
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
    }

    false
}

/// Updates fragment definition indices
fn update_definition_indices(
    fragment_definitions: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_fragments: Option<Vec<String>>,
    new_fragments: Vec<String>,
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
    fragment_dependents: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_spreads: Option<Vec<String>>,
    new_spreads: Vec<String>,
) {
    super::fragment_manager::update_fragment_dependents(
        fragment_dependents,
        uri,
        old_spreads,
        new_spreads,
    );
}
