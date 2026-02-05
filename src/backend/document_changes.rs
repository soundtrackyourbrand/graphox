//! Document change handling
//!
//! This module handles incremental document updates from LSP
//! didChange notifications, including fragment tracking and
//! affected document computation.

use crate::document::DocumentState;
use dashmap::DashMap;
use fnv::FnvHashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

/// Result of processing document changes
pub struct ChangeResult {
    pub uris_to_validate: Vec<Url>,
}

/// Processes document content changes and updates indices
pub fn process_document_change(
    uri: &Url,
    changes: Vec<TextDocumentContentChangeEvent>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    fragment_defs: &Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    fragment_spreads: &Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    fragment_dependents: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    fragment_definitions: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
) -> Option<ChangeResult> {
    let mut affected_fragment_names = FnvHashSet::default();
    let mut old_fragment_names = Vec::new();
    let old_spreads: Vec<String>;

    let new_fragments: Vec<crate::document::FragmentDef>;
    let new_spreads: Vec<String>;
    let package_root: Option<PathBuf>;
    let new_fragment_names: Vec<String>;
    let affected_spread_names: FnvHashSet<String>;

    // Get document and apply changes
    if let Some(doc_arc) = documents.get(uri).map(|r| r.value().clone()) {
        let mut doc = (*doc_arc).clone();

        // Collect fragments before change
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
            old_fragment_names.push(f.name.clone());
        }

        old_spreads = doc.fragment_spreads.clone();

        // Apply changes incrementally
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&doc.language.get_parser_language())
            .unwrap();

        for change in changes {
            doc.apply_change(&change, &mut parser);
        }

        // Collect fragments after change
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
        }

        new_fragments = doc.fragments().to_vec();
        new_spreads = doc.fragment_spreads.clone();

        // Compute affected spreads (added or removed)
        let mut aff_spreads = FnvHashSet::default();
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

        documents.insert(uri.clone(), Arc::new(doc));
    } else {
        return None;
    }

    // Update indices
    fragment_defs.insert(uri.clone(), new_fragments);
    fragment_spreads.insert(uri.clone(), new_spreads.clone());

    super::fragment_manager::update_fragment_dependents(
        fragment_dependents,
        uri,
        Some(old_spreads),
        new_spreads,
    );

    super::fragment_manager::update_fragment_definitions(
        fragment_definitions,
        uri,
        Some(old_fragment_names),
        new_fragment_names,
    );

    package_roots.insert(uri.clone(), package_root);

    // Compute affected URIs
    let uris_to_validate = super::validation::get_affected_uris(
        uri.clone(),
        affected_fragment_names,
        affected_spread_names,
        documents,
        fragment_dependents,
        fragment_definitions,
    );

    Some(ChangeResult { uris_to_validate })
}
