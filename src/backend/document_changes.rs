//! Document change handling
//!
//! This module handles incremental document updates from LSP
//! didChange notifications, including fragment tracking and
//! affected document computation.

use crate::types::{
    DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap, FragmentDependentsMap,
    FragmentSpreadsMap, PackageRootsMap,
};
use ahash::AHashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

/// Result of processing document changes
pub struct ChangeResult {
    pub uris_to_validate: Vec<Url>,
}

/// Parameters for processing document changes
pub struct DocumentChangeParams<'a> {
    pub documents: &'a DocumentsMap,
    pub fragment_defs: &'a FragmentDefsMap,
    pub fragment_spreads: &'a FragmentSpreadsMap,
    pub package_roots: &'a PackageRootsMap,
    pub fragment_dependents: &'a FragmentDependentsMap,
    pub fragment_definitions: &'a FragmentDefinitionsMap,
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
    let old_spreads: Vec<String>;

    let new_fragments: Vec<crate::document::FragmentDef>;
    let new_spreads: Vec<String>;
    let package_root: Option<PathBuf>;
    let new_fragment_names: Vec<String>;
    let affected_spread_names: AHashSet<String>;

    // Get document and apply changes
    if let Some(doc_arc) = params.documents.get(uri).map(|r| r.value().clone()) {
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
            doc.apply_change(&change, &mut parser, version);
        }

        // Collect fragments after change
        for f in doc.fragments() {
            affected_fragment_names.insert(f.name.clone());
        }

        new_fragments = doc.fragments().to_vec();
        new_spreads = doc.fragment_spreads.clone();

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

    // Compute affected URIs
    let uris_to_validate = super::validation::get_affected_uris(
        uri.clone(),
        affected_fragment_names,
        affected_spread_names,
        params.documents,
        params.fragment_dependents,
        params.fragment_definitions,
    );

    Some(ChangeResult { uris_to_validate })
}
