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

use crate::backend::helpers::{named_operation_names, update_operation_name_index};

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
        return process_file_deleted(uri, params, |u| u);
    }

    let mut affected_fragment_names = AHashSet::default();
    let mut affected_spread_names = AHashSet::default();
    let mut affected_operation_names = AHashSet::default();

    let old_fragment_defs: Option<Arc<[graphox_core::document::FragmentDef]>> =
        params.metadata.get(&uri).map(|m| m.fragments.clone());
    let old_fragment_names: Option<Arc<[Arc<str>]>> = old_fragment_defs
        .as_ref()
        .map(|defs| defs.iter().map(|f| f.name.clone()).collect::<Arc<[_]>>());
    let old_spreads: Option<Arc<[Arc<str>]>> = params
        .metadata
        .get(&uri)
        .map(|m| m.fragment_spreads.clone());
    let old_operation_names: Option<Arc<[Arc<str>]>> = params
        .metadata
        .get(&uri)
        .map(|m| named_operation_names(&m.operations));

    let new_fragment_defs = new_doc.fragments.clone();
    let new_fragment_names: Arc<[Arc<str>]> =
        new_fragment_defs.iter().map(|f| f.name.clone()).collect();
    let new_spreads: Arc<[Arc<str>]> = new_doc.fragment_spreads.clone();

    // Track changes to fragment definitions: additions, removals, and body edits.
    // Body edits are matched by source hash and are the common case for a watched
    // change (a pull or branch switch rewriting a fragment in place); missing them
    // would leave the fragment's consumers unvalidated and their codegen stale.
    if let Some(old) = &old_fragment_names {
        for name in old.iter() {
            if !new_fragment_names.contains(name) {
                affected_fragment_names.insert(name.clone());
            }
        }
    }
    for new_frag in new_fragment_defs.iter() {
        let unchanged = old_fragment_defs
            .as_ref()
            .and_then(|defs| defs.iter().find(|f| f.name == new_frag.name))
            .is_some_and(|old_frag| old_frag.source_hash == new_frag.source_hash);
        if !unchanged {
            affected_fragment_names.insert(new_frag.name.clone());
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
        old_fragment_names,
        new_fragment_names,
    );
    super::fragment_manager::update_fragment_dependents(
        params.fragment_dependents,
        &uri,
        old_spreads,
        new_spreads,
    );

    affected_operation_names.extend(update_operation_name_index(
        params.operation_names,
        params.config,
        &uri,
        old_operation_names.as_deref(),
        new_doc.operations(),
    ));

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
    let old_operation_names: Option<Arc<[Arc<str>]>> = params
        .metadata
        .get(&uri)
        .map(|m| named_operation_names(&m.operations));

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

    affected_operation_names.extend(update_operation_name_index(
        params.operation_names,
        params.config,
        &uri,
        old_operation_names.as_deref(),
        &[],
    ));

    // If the deleted file contributed any GraphQL (fragments, spreads, or
    // operations), regenerate its closure: the deleted file's own project (so a
    // bundle no longer includes the removed operations) and any project that
    // consumed a fragment it defined (so cross-project consumers don't keep stale
    // generated types referencing the now-missing fragment).
    let had_graphql_content = !affected_fragment_names.is_empty()
        || !affected_spread_names.is_empty()
        || !affected_operation_names.is_empty();

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
        should_run_codegen: had_graphql_content,
        should_reload_config: false,
    })
}

/// Checks if a path is the config file
pub(crate) fn is_config_file(path: &std::path::Path, config: &graphox_core::Config) -> bool {
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
