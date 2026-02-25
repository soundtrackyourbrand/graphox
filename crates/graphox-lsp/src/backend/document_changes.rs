//! Document change handling
//!
//! This module handles incremental document updates from LSP
//! didChange notifications, including fragment tracking and
//! affected document computation.

use ahash::AHashSet;
use graphox_core::types::{
    DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap, FragmentDependentsMap,
    FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

/// Result of processing document changes
pub struct ChangeResult {
    pub uris_to_validate: Vec<Url>,
    pub should_run_codegen: bool,
}

/// Parameters for processing document changes
pub struct DocumentChangeParams<'a> {
    pub documents: &'a DocumentsMap,
    pub fragment_defs: &'a FragmentDefsMap,
    pub fragment_spreads: &'a FragmentSpreadsMap,
    pub package_roots: &'a PackageRootsMap,
    pub fragment_dependents: &'a FragmentDependentsMap,
    pub fragment_definitions: &'a FragmentDefinitionsMap,
    pub operation_names: &'a OperationNamesMap,
    pub config: &'a graphox_core::Config,
    pub position_encoding: PositionEncodingKind,
}

/// Processes document content changes and updates indices
pub fn process_document_change(
    uri: &Url,
    changes: Vec<TextDocumentContentChangeEvent>,
    version: i32,
    params: &DocumentChangeParams<'_>,
) -> Option<ChangeResult> {
    let mut affected_fragment_names = AHashSet::default();
    let mut old_fragment_names = Vec::new();
    let mut affected_operation_names = AHashSet::default();
    let old_spreads: Vec<Arc<str>>;
    let had_graphql_before: bool;
    let has_graphql_after: bool;

    let new_fragments: Vec<graphox_core::document::FragmentDef>;
    let new_spreads: Vec<Arc<str>>;
    let package_root: Option<PathBuf>;
    let new_fragment_names: Vec<Arc<str>>;
    let affected_spread_names: AHashSet<Arc<str>>;

    // Get document and apply changes
    if let Some(doc_arc) = params.documents.get(uri).map(|r| r.value().clone()) {
        let mut doc = (*doc_arc).clone();
        had_graphql_before = !doc.get_graphql_trees().is_empty();

        // Collect fragments before change
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
            old_fragment_names.push(f.name.clone());
        }

        old_spreads = doc.fragment_spreads.clone();

        for change in changes {
            doc.apply_change_from_thread_local(&change, version);
        }

        // Collect fragments after change
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
        }

        new_fragments = doc.fragments().to_vec();
        new_spreads = doc.fragment_spreads.clone();
        has_graphql_after = !doc.get_graphql_trees().is_empty();

        // Compute affected spreads (added or removed)
        let mut aff_spreads = AHashSet::default();
        for s in &old_spreads {
            if !new_spreads.contains(s) {
                aff_spreads.insert(s.clone());
            }
        }
        for s in &new_spreads {
            if !old_spreads.contains(s) {
                aff_spreads.insert(s.clone());
            }
        }
        affected_spread_names = aff_spreads;
        package_root = doc.package_root.clone();
        new_fragment_names = doc.fragments().iter().map(|f| f.name.clone()).collect();

        params.documents.insert(uri.clone(), Arc::new(doc));
    } else {
        return None;
    }

    // Update indices
    params.fragment_defs.insert(uri.clone(), new_fragments);
    params
        .fragment_spreads
        .insert(uri.clone(), new_spreads.clone());

    super::fragment_manager::update_fragment_dependents(
        params.fragment_dependents,
        uri,
        Some(old_spreads),
        new_spreads,
    );

    super::fragment_manager::update_fragment_definitions(
        params.fragment_definitions,
        uri,
        Some(old_fragment_names),
        new_fragment_names,
    );

    params.package_roots.insert(uri.clone(), package_root);

    // Update operation names index
    if let Ok(path) = uri.to_file_path()
        && let Some(schema_key) = params.config.get_schema_for_path(&path)
    {
        let project_key = params
            .config
            .get_project_for_path(&path)
            .map(|p| p.include().as_key())
            .unwrap_or_else(|| schema_key);
        let project_key_arc: Arc<str> = project_key.into();

        // Remove old entries for this URI
        for mut entry in params.operation_names.iter_mut() {
            let op_name = entry.key().clone();
            let mut removed = false;
            entry.value_mut().retain(|(_, op_uri)| {
                if op_uri == uri {
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

        // Add new entries
        if let Some(doc) = params.documents.get(uri) {
            for op in doc.operations() {
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
    }

    // Compute affected URIs
    let uris_to_validate = super::validation::get_affected_uris(
        uri.clone(),
        affected_fragment_names,
        affected_spread_names,
        affected_operation_names,
        params.documents,
        params.fragment_dependents,
        params.fragment_definitions,
        params.operation_names,
    );

    Some(ChangeResult {
        uris_to_validate,
        should_run_codegen: had_graphql_before || has_graphql_after,
    })
}
