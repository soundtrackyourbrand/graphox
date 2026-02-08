//! Fragment metadata collection and management
//!
//! This module extracts the fragment-related logic that was duplicated in
//! Backend::initialized and Backend::get_all_fragments_info

use ahash::AHashSet;
use dashmap::DashMap;
use graphql_core::config::Config;
use graphql_core::document::FragmentDef;
use graphql_features::completion::FragmentCompletionInfo;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::Url;

/// Collects fragment metadata from fragment definitions
///
/// This was previously duplicated in:
/// - Backend::get_all_fragments_info (backend.rs:131-171)
/// - Backend::initialized workspace scan (backend.rs:926-967)
pub fn collect_fragment_metadata(
    fragment_defs: &Arc<DashMap<Url, Vec<FragmentDef>, ahash::RandomState>>,
    config: &Config,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
) -> Vec<FragmentCompletionInfo> {
    // Clone Arc references to avoid holding locks during iteration
    // This prevents lock contention when accessing multiple DashMaps concurrently
    let fragment_defs = fragment_defs.clone();
    let package_roots = package_roots.clone();

    fragment_defs
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let frags = entry.value();

            // Get project info once per file
            let (import_path, package_root) = if let Ok(p) = uri.to_file_path() {
                let project = config.get_project_for_path(&p);
                (
                    project.and_then(|proj| proj.import.clone()),
                    package_roots.get(uri).and_then(|r| r.value().clone()),
                )
            } else {
                (None, None)
            };

            frags
                .iter()
                .map(move |frag| FragmentCompletionInfo {
                    name: frag.name.clone(),
                    type_condition: frag.type_condition.clone(),
                    description: frag.description.clone(),
                    import_path: import_path.clone(),
                    is_public: frag.is_public,
                    is_type_only: frag.is_type_only,
                    uri: uri.clone(),
                    package_root: package_root.clone(),
                    used_variables: frag.used_variables.clone(),
                    used_fragments: frag.used_fragments.clone(),
                    requirements: std::collections::BTreeMap::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Collects fragment metadata with schema information
///
/// Returns tuples of (FragmentCompletionInfo, Option<schema_key>)
/// Used during workspace scanning for validation
pub fn collect_fragment_metadata_with_schema(
    fragment_defs: &Arc<DashMap<Url, Vec<FragmentDef>, ahash::RandomState>>,
    config: &Config,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
) -> Vec<(FragmentCompletionInfo, Option<String>)> {
    // Clone Arc references to avoid holding locks during iteration
    // This prevents lock contention when accessing multiple DashMaps concurrently
    let fragment_defs = fragment_defs.clone();
    let package_roots = package_roots.clone();

    fragment_defs
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let frags = entry.value();

            let (import_path, schema_key) = if let Ok(p) = uri.to_file_path() {
                let project = config.get_project_for_path(&p);
                (
                    project.and_then(|proj| proj.import.clone()),
                    project.map(|proj| proj.schema.as_key()),
                )
            } else {
                (None, None)
            };

            let package_root = package_roots.get(uri).and_then(|r| r.value().clone());

            frags
                .iter()
                .map(move |frag| {
                    (
                        FragmentCompletionInfo {
                            name: frag.name.clone(),
                            type_condition: frag.type_condition.clone(),
                            description: frag.description.clone(),
                            import_path: import_path.clone(),
                            is_public: frag.is_public,
                            is_type_only: frag.is_type_only,
                            uri: uri.clone(),
                            package_root: package_root.clone(),
                            used_variables: frag.used_variables.clone(),
                            used_fragments: frag.used_fragments.clone(),
                            requirements: std::collections::BTreeMap::new(),
                        },
                        schema_key.clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Updates the fragment dependent index when fragments change
pub fn update_fragment_dependents(
    fragment_dependents: &Arc<DashMap<String, AHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_spreads: Option<Vec<String>>,
    new_spreads: Vec<String>,
) {
    if let Some(old) = old_spreads {
        for spread in old {
            if !new_spreads.contains(&spread)
                && let Some(mut entry) = fragment_dependents.get_mut(&spread)
            {
                entry.value_mut().remove(uri);
            }
        }
    }

    for spread in new_spreads {
        fragment_dependents
            .entry(spread)
            .or_default()
            .insert(uri.clone());
    }
}

/// Updates the fragment definition index when fragments are added/removed
pub fn update_fragment_definitions(
    fragment_definitions: &Arc<DashMap<String, AHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_fragments: Option<Vec<String>>,
    new_fragments: Vec<String>,
) {
    if let Some(old) = old_fragments {
        for name in old {
            if !new_fragments.contains(&name)
                && let Some(mut entry) = fragment_definitions.get_mut(&name)
            {
                entry.value_mut().remove(uri);
            }
        }
    }

    for name in new_fragments {
        fragment_definitions
            .entry(name)
            .or_default()
            .insert(uri.clone());
    }
}
