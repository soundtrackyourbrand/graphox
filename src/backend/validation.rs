//! Document validation operations
//!
//! This module handles all validation-related operations including
//! validating individual URIs, computing affected documents, and
//! publishing diagnostics.

use crate::config::Config;
use crate::document::DocumentState;
use crate::features::completion::FragmentCompletionInfo;
use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use dashmap::{DashMap, DashSet};
use fnv::FnvHashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::lsp_types::*;
use tower_lsp::Client;

/// Parameters for validation operations
pub struct ValidationParams<'a> {
    pub client: &'a Client,
    pub documents: &'a Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    pub config: &'a Config,
    pub fragment_defs: &'a Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    pub fragment_spreads: &'a Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    pub package_roots: &'a Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    pub validated_schemas: &'a Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    pub valid_empty_schema: &'a Arc<Valid<Schema>>,
    pub workspace_loaded: &'a Arc<AtomicBool>,
    pub open_documents: &'a Arc<DashSet<Url, ahash::RandomState>>,
    pub fragment_dependents: &'a Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub fragment_definitions: &'a Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
}

/// Validates a list of document URIs and publishes diagnostics
pub async fn validate_uris(params: ValidationParams<'_>, uris: Vec<Url>) {
    if uris.is_empty() {
        return;
    }

    let mut to_publish = Vec::new();
    let used_fragments = get_used_fragments(params.fragment_spreads);
    let workspace_loaded = params.workspace_loaded.load(Ordering::SeqCst);

    for uri in uris {
        if let Some(doc) = params.documents.get(&uri).map(|r| r.value().clone()) {
            // Skip validating schema files as executable documents
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = params.config.get_schema_for_path(&path)
                    && schema_key.contains(&path.to_string_lossy().to_string())
                        && !params.open_documents.contains(&uri)
                    {
                        continue;
                    }

            let schema = get_schema_for_doc(&uri, params.config, params.validated_schemas, params.valid_empty_schema);
            let filtered_fragments = get_fragments_for_doc(
                &doc,
                params.config,
                params.fragment_defs,
                params.package_roots,
            );

            let diagnostics = doc.get_semantic_diagnostics(
                &schema,
                &filtered_fragments,
                Some(&used_fragments),
                Some(params.config),
                false,
                workspace_loaded,
            );
            to_publish.push((uri.clone(), diagnostics));
        }
    }

    for (u, d) in to_publish {
        params.client.publish_diagnostics(u, d, None).await;
    }
}

/// Validates all documents in the workspace
pub async fn validate_all_documents(params: ValidationParams<'_>) {
    let all_uris: Vec<Url> = params.documents.iter().map(|e| e.key().clone()).collect();
    validate_uris(params, all_uris).await;
}

/// Computes the set of URIs that need validation based on affected fragments
pub fn get_affected_uris(
    initial_uri: Url,
    affected_fragment_names: FnvHashSet<String>,
    affected_spread_names: FnvHashSet<String>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    fragment_dependents: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    fragment_definitions: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
) -> Vec<Url> {
    let mut uris_to_validate = FnvHashSet::default();
    uris_to_validate.insert(initial_uri);

    let mut to_process: Vec<String> = affected_fragment_names.into_iter().collect();
    let mut processed_fragments = FnvHashSet::default();

    while let Some(frag_name) = to_process.pop() {
        if !processed_fragments.insert(frag_name.clone()) {
            continue;
        }

        if let Some(dependents) = fragment_dependents.get(&frag_name) {
            for dep_uri in dependents.value() {
                if uris_to_validate.insert(dep_uri.clone())
                    && let Some(doc) = documents.get(dep_uri).map(|r| r.value().clone()) {
                        for f in doc.fragments() {
                            to_process.push(f.name.clone());
                        }
                    }
            }
        }
    }

    for spread_name in affected_spread_names {
        if let Some(definitions) = fragment_definitions.get(&spread_name) {
            for def_uri in definitions.value() {
                uris_to_validate.insert(def_uri.clone());
            }
        }
    }

    uris_to_validate.into_iter().collect()
}

/// Gets all used fragments across the workspace
pub fn get_used_fragments(
    fragment_spreads: &Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
) -> FnvHashSet<String> {
    let mut used = FnvHashSet::default();
    for entry in fragment_spreads.iter() {
        for spread in entry.value() {
            used.insert(spread.clone());
        }
    }
    used
}

/// Gets the schema for a given document URI
pub fn get_schema_for_doc(
    uri: &Url,
    config: &Config,
    validated_schemas: &Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    valid_empty_schema: &Arc<Valid<Schema>>,
) -> Arc<Valid<Schema>> {
    if let Ok(path) = uri.to_file_path()
        && let Some(schema_path) = config.get_schema_for_path(&path)
        && let Some(schema) = validated_schemas.get(&schema_path)
    {
        return schema.value().clone();
    }

    valid_empty_schema.clone()
}

/// Gets fragments available for a given document
pub fn get_fragments_for_doc(
    doc: &DocumentState,
    config: &Config,
    fragment_defs: &Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
) -> Vec<FragmentCompletionInfo> {
    let all_fragments = super::fragment_manager::collect_fragment_metadata(
        fragment_defs,
        config,
        package_roots,
    );

    let target_package_root = doc.package_root.as_ref();
    let doc_path = doc.uri.to_file_path().ok();
    let schema_key = doc_path
        .as_ref()
        .and_then(|p| config.get_schema_for_path(p));

    let mut filtered: Vec<_> = all_fragments
        .into_iter()
        .filter(|f| {
            let is_same_package = f.package_root.as_ref() == target_package_root;
            if is_same_package || f.is_public {
                return true;
            }

            if let Ok(f_path) = f.uri.to_file_path() {
                let f_schema_key = config.get_schema_for_path(&f_path);
                return f_schema_key.is_some() && f_schema_key == schema_key;
            }
            false
        })
        .collect();

    // Prioritize fragments from same package
    filtered.sort_by(|a, b| {
        let a_same_pkg = a.package_root.as_ref() == target_package_root;
        let b_same_pkg = b.package_root.as_ref() == target_package_root;

        if a_same_pkg != b_same_pkg {
            return b_same_pkg.cmp(&a_same_pkg);
        }

        b.is_public.cmp(&a.is_public).reverse()
    });

    filtered
}
