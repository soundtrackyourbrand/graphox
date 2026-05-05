//! Document change handling
//!
//! This module handles incremental document updates from LSP
//! didChange notifications, including fragment tracking and
//! affected document computation.

use ahash::AHashSet;
use graphox_core::types::{
    DocumentsMap, FragmentDefinitionsMap, FragmentDependentsMap, MetadataMap, OperationNamesMap,
};
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

use crate::backend::helpers::{named_operation_names, update_operation_name_index};

/// Result of processing document changes
pub struct ChangeResult {
    pub uris_to_validate: Vec<Url>,
    pub should_run_codegen: bool,
}

/// Parameters for processing document changes
pub struct DocumentChangeParams<'a> {
    pub documents: &'a DocumentsMap,
    pub metadata: &'a MetadataMap,
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
    let old_fragment_names: Arc<[Arc<str>]>;
    let mut affected_operation_names = AHashSet::default();
    let old_spreads: Arc<[Arc<str>]>;
    let old_operation_names: Arc<[Arc<str>]>;

    let new_fragments: Arc<[graphox_core::document::FragmentDef]>;
    let new_spreads: Arc<[Arc<str>]>;
    let package_root: Option<PathBuf>;
    let new_fragment_names: Arc<[Arc<str>]>;
    let mut affected_spread_names: AHashSet<Arc<str>>;
    let new_operations: Arc<[graphox_core::document::OperationDef]>;
    let new_version: i32;

    if let Some(mut doc_ref) = params.documents.get_mut(uri) {
        let doc = Arc::make_mut(&mut doc_ref);

        // Track old state for indices
        old_fragment_names = doc.fragments.iter().map(|f| f.name.clone()).collect();
        old_spreads = doc.fragment_spreads.clone();
        old_operation_names = named_operation_names(&doc.operations);
        let old_fragments = doc.fragments.clone();

        for change in &changes {
            doc.apply_change_from_thread_local(change, version);
        }

        new_fragments = doc.fragments.clone();
        new_spreads = doc.fragment_spreads.clone();
        package_root = doc.package_root.clone();
        new_fragment_names = new_fragments.iter().map(|f| f.name.clone()).collect();
        new_operations = doc.operations.clone();
        new_version = doc.version;

        // Compute affected fragments
        for name in old_fragment_names.iter() {
            if !new_fragment_names.contains(name) {
                affected_fragment_names.insert(name.clone());
            }
        }
        for name in new_fragment_names.iter() {
            if !old_fragment_names.contains(name) {
                affected_fragment_names.insert(name.clone());
            } else if let Some(old_frag) = old_fragments.iter().find(|f| &f.name == name)
                && let Some(new_frag) = new_fragments.iter().find(|f| &f.name == name)
                && old_frag.source_hash != new_frag.source_hash
            {
                affected_fragment_names.insert(name.clone());
            }
        }

        // Compute affected spreads
        let mut spreads_set = AHashSet::default();
        for s in old_spreads.iter() {
            spreads_set.insert(s.clone());
        }
        affected_spread_names = AHashSet::default();
        for s in new_spreads.iter() {
            if !spreads_set.contains(s) {
                affected_spread_names.insert(s.clone());
            }
        }
        let mut new_spreads_set = AHashSet::default();
        for s in new_spreads.iter() {
            new_spreads_set.insert(s.clone());
        }
        for s in old_spreads.iter() {
            if !new_spreads_set.contains(s) {
                affected_spread_names.insert(s.clone());
            }
        }
    } else {
        return None;
    }

    // Update metadata
    let metadata = Arc::new(graphox_core::types::DocumentMetadata {
        fragments: new_fragments,
        fragment_spreads: new_spreads.clone(),
        package_root,
        operations: new_operations.clone(),
        version: new_version,
    });
    params.metadata.insert(uri.clone(), metadata);

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

    affected_operation_names.extend(update_operation_name_index(
        params.operation_names,
        params.config,
        uri,
        Some(old_operation_names.as_ref()),
        &new_operations,
    ));

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
        should_run_codegen: false,
    })
}
