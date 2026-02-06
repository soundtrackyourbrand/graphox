//! Document validation operations
//!
//! This module handles all validation-related operations including
//! validating individual URIs, computing affected documents, and
//! publishing diagnostics.

use crate::config::Config;
use crate::document::DocumentState;
use crate::features::completion::FragmentCompletionInfo;
use crate::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap,
    FragmentDependentsMap, FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use dashmap::{DashMap, DashSet};
use fnv::FnvHashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Type alias for diagnostic cache
pub type DiagnosticCache = DiagnosticCacheMap;

/// Parameters for validation operations
pub struct ValidationParams<'a> {
    pub client: &'a Client,
    pub documents: &'a DocumentsMap,
    pub config: &'a Config,
    pub fragment_defs: &'a FragmentDefsMap,
    pub fragment_spreads: &'a FragmentSpreadsMap,
    pub package_roots: &'a PackageRootsMap,
    pub validated_schemas: &'a Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    pub valid_empty_schema: &'a Arc<Valid<Schema>>,
    pub workspace_loaded: &'a Arc<AtomicBool>,
    pub open_documents: &'a Arc<DashSet<Url, ahash::RandomState>>,
    pub fragment_dependents: &'a FragmentDependentsMap,
    pub fragment_definitions: &'a FragmentDefinitionsMap,
    pub operation_names: &'a OperationNamesMap,
    pub supports_progress: bool,
}

/// Validates a list of document URIs and publishes diagnostics
///
/// If `use_push` is true, diagnostics are pushed via `publishDiagnostics`.
/// If false, diagnostics are only cached for pull-based retrieval.
pub async fn validate_uris(
    params: ValidationParams<'_>,
    uris: Vec<Url>,
    use_push: bool,
    diagnostic_cache: Option<&DiagnosticCache>,
) {
    if uris.is_empty() {
        return;
    }

    // Create progress reporter if validating multiple documents
    let progress = if uris.len() > 5 {
        Some(
            super::progress::ProgressReporter::new(
                params.client.clone(),
                format!("Validating {} documents", uris.len()),
                params.supports_progress,
            )
            .await,
        )
    } else {
        None
    };

    let mut to_publish = Vec::new();
    let used_fragments = get_used_fragments(params.fragment_spreads);
    let workspace_loaded = params.workspace_loaded.load(Ordering::SeqCst);
    let total = uris.len();

    for (idx, uri) in uris.into_iter().enumerate() {
        if let Some(doc) = params.documents.get(&uri).map(|r| r.value().clone()) {
            // Skip validating schema files as executable documents
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = params.config.get_schema_for_path(&path)
                && schema_key.contains(&path.to_string_lossy().to_string())
                && !params.open_documents.contains(&uri)
            {
                continue;
            }

            let schema = get_schema_for_doc(
                &uri,
                params.config,
                params.validated_schemas,
                params.valid_empty_schema,
            );
            let filtered_fragments = get_fragments_for_doc(
                &doc,
                params.config,
                params.fragment_defs,
                params.package_roots,
            );

            let mut diagnostics = doc.get_semantic_diagnostics(
                &schema,
                &filtered_fragments,
                Some(&used_fragments),
                Some(params.config),
                false,
                workspace_loaded,
            );

            // Add duplicate operation name diagnostics if enabled
            if let Some(rules) = &params.config.rules
                && let Some(true) = rules.unique_operation_name
                && let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = params.config.get_schema_for_path(&path)
            {
                add_duplicate_operation_diagnostics(
                    &doc,
                    &uri,
                    &schema_key,
                    params.operation_names,
                    &mut diagnostics,
                );
            }

            // Cache diagnostics for pull-based diagnostics
            if let Some(cache) = diagnostic_cache {
                cache.insert(uri.clone(), (doc.version, diagnostics.clone()));
            }

            if use_push {
                to_publish.push((uri.clone(), diagnostics));
            }

            // Report progress
            if let Some(ref p) = progress {
                let percentage = ((idx + 1) * 100 / total) as u32;
                p.report(
                    format!("Validated {}/{} documents", idx + 1, total),
                    Some(percentage),
                )
                .await;
            }
        }
    }

    // End progress
    if let Some(p) = progress {
        p.end(Some(format!("Validated {} documents", total))).await;
    }

    // Only publish if using push-based diagnostics
    if use_push {
        for (u, d) in to_publish {
            params.client.publish_diagnostics(u, d, None).await;
        }
    }
}

/// Validates all documents in the workspace
pub async fn validate_all_documents(
    params: ValidationParams<'_>,
    use_push: bool,
    diagnostic_cache: Option<&DiagnosticCache>,
) {
    let all_uris: Vec<Url> = params.documents.iter().map(|e| e.key().clone()).collect();
    validate_uris(params, all_uris, use_push, diagnostic_cache).await;
}

/// Computes the set of URIs that need validation based on affected fragments
pub fn get_affected_uris(
    initial_uri: Url,
    affected_fragment_names: FnvHashSet<String>,
    affected_spread_names: FnvHashSet<String>,
    documents: &DocumentsMap,
    fragment_dependents: &FragmentDependentsMap,
    fragment_definitions: &FragmentDefinitionsMap,
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
                    && let Some(doc) = documents.get(dep_uri).map(|r| r.value().clone())
                {
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
pub fn get_used_fragments(fragment_spreads: &FragmentSpreadsMap) -> FnvHashSet<String> {
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
    fragment_defs: &FragmentDefsMap,
    package_roots: &PackageRootsMap,
) -> Vec<FragmentCompletionInfo> {
    let all_fragments =
        super::fragment_manager::collect_fragment_metadata(fragment_defs, config, package_roots);

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

/// Adds diagnostics for duplicate operation names within the same project
fn add_duplicate_operation_diagnostics(
    doc: &DocumentState,
    uri: &Url,
    schema_key: &str,
    operation_names: &OperationNamesMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check each operation in this document
    for op in doc.operations() {
        if let Some(name) = &op.name {
            // Look up this operation name in the index
            if let Some(entry) = operation_names.get(name) {
                // Filter to only operations in the same project (same schema)
                let locations_in_project: Vec<&Url> = entry
                    .value()
                    .iter()
                    .filter(|(schema, _)| schema == schema_key)
                    .map(|(_, uri)| uri)
                    .collect();

                // If there are multiple locations in this project, it's a duplicate
                if locations_in_project.len() > 1 {
                    // Find the position of the operation in this document
                    let range = find_operation_range(doc, name).unwrap_or(Range {
                        start: Position::new(0, 0),
                        end: Position::new(0, 0),
                    });

                    // Build list of other files
                    let other_files: Vec<String> = locations_in_project
                        .iter()
                        .filter(|loc| **loc != uri)
                        .filter_map(|loc| loc.to_file_path().ok())
                        .map(|path| path.display().to_string())
                        .collect();

                    push_duplicate_operation_diagnostic(
                        diagnostics,
                        range,
                        name,
                        if other_files.is_empty() {
                            None
                        } else {
                            Some(other_files)
                        },
                    );

                    // Only report once per operation name in this file
                    break;
                }
            }
        }
    }
}

pub(crate) fn push_duplicate_operation_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    range: Range,
    name: &str,
    other_files: Option<Vec<String>>,
) {
    let message = if let Some(files) = other_files {
        format!(
            "Duplicate operation name '{}' (also in: {})",
            name,
            files.join(", ")
        )
    } else {
        format!("Duplicate operation name '{}'", name)
    };

    diagnostics.push(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        message,
        code: Some(NumberOrString::String("duplicate_operation".to_string())),
        ..Default::default()
    });
}

/// Finds the range of an operation definition by name
fn find_operation_range(doc: &DocumentState, operation_name: &str) -> Option<Range> {
    use crate::queries::*;
    use tree_sitter::{QueryCursor, StreamingIterator};

    for block in doc.get_graphql_trees() {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut matches =
            cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                doc.rope
                    .byte_slice(
                        (node.start_byte() + block.offset)..(node.end_byte() + block.offset),
                    )
                    .chunks()
            });

        while let Some(m) = matches.next() {
            let mut name = None;
            let mut op_node = None;

            for cap in m.captures {
                let cap_name = query.capture_names()[cap.index as usize];
                if cap_name == "symbol.name" {
                    name = Some(doc.get_node_text(cap.node, block.offset));
                } else if cap_name == "symbol.full" && cap.node.kind() == "operation_definition" {
                    op_node = Some(cap.node);
                }
            }

            if let (Some(n), Some(node)) = (name, op_node)
                && n == operation_name
            {
                return Some(doc.translate_to_file_range(node, block.offset));
            }
        }
    }

    None
}
