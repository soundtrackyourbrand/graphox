//! Document validation operations
//!
//! This module handles all validation-related operations including
//! validating individual URIs, computing affected documents, and
//! publishing diagnostics.

use ahash::{AHashMap, AHashSet};
use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use dashmap::{DashMap, DashSet};
use graphox_core::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDependentsMap, MetadataMap,
    OperationNamesMap,
};
use graphox_core::utils::{find_operation_range, push_duplicate_operation_diagnostic};
use graphox_core::{Config, DocumentState};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::diagnostics::DocumentDiagnostics;
use rayon::prelude::*;
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
    pub metadata: &'a MetadataMap,
    pub validated_schemas: &'a Arc<DashMap<String, Arc<Valid<Schema>>, ahash::RandomState>>,
    pub valid_empty_schema: &'a Arc<Valid<Schema>>,
    pub workspace_loaded: &'a Arc<AtomicBool>,
    pub open_documents: &'a Arc<DashSet<Url, ahash::RandomState>>,
    pub fragment_dependents: &'a FragmentDependentsMap,
    pub fragment_definitions: &'a FragmentDefinitionsMap,
    pub operation_names: &'a OperationNamesMap,
    pub subgraphs:
        &'a Arc<DashMap<String, Vec<graphox_core::schema::SubgraphInfo>, ahash::RandomState>>,
    pub schemas: &'a Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
    pub supports_progress: bool,
    pub position_encoding: PositionEncodingKind,
    pub result_id_epoch: usize,
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

    let used_fragments = get_used_fragments(params.metadata);
    let workspace_loaded = params.workspace_loaded.load(Ordering::SeqCst);
    let total = uris.len();

    // Collect fragment metadata once for all documents being validated
    let all_fragments = super::fragment_manager::collect_fragment_metadata(
        params.metadata,
        params.config,
        params.subgraphs,
        params.documents,
        params.schemas,
    );

    // Pre-collect documents and their package roots to optimize fragment filtering
    let mut docs_to_validate = Vec::new();
    let mut unique_package_roots = AHashSet::new();

    for uri in uris {
        if let Some(doc) = params.documents.get(&uri).map(|r| r.value().clone()) {
            // Skip validating schema files as executable documents
            if let Ok(path) = uri.to_file_path() {
                // Check if this file is used as a schema in any project
                let is_schema = params.config.projects().iter().any(|p| {
                    p.schema().files().iter().any(|f| {
                        let abs_schema = params.config.base_dir().join(f);
                        graphox_core::utils::paths_match(Some(&path), Some(&abs_schema))
                    })
                }) || params.config.schema_types().iter().any(|st| {
                    st.schema().files().iter().any(|f| {
                        let abs_schema = params.config.base_dir().join(f);
                        graphox_core::utils::paths_match(Some(&path), Some(&abs_schema))
                    })
                });

                if is_schema && !params.open_documents.contains(&uri) {
                    continue;
                }
            }

            unique_package_roots.insert(doc.package_root.clone());
            docs_to_validate.push((uri, doc));
        }
    }

    // Pre-calculate filtered fragments for each unique package root
    let mut fragments_by_pkg = AHashMap::with_capacity(unique_package_roots.len());
    for pkg_root in unique_package_roots {
        let filtered = get_fragments_for_doc_with_metadata(pkg_root.as_deref(), &all_fragments);
        fragments_by_pkg.insert(pkg_root, Arc::new(filtered));
    }

    // Process documents in parallel
    let results: Vec<_> = docs_to_validate
        .into_par_iter()
        .map(|(uri, doc)| {
            let schema = get_schema_for_doc(
                &uri,
                params.config,
                params.validated_schemas,
                params.valid_empty_schema,
            );

            let filtered_fragments = fragments_by_pkg
                .get(&doc.package_root)
                .expect("Package root should be in cache");

            // Use project-specific rules if defined, otherwise fall back to global rules
            let project_config = uri
                .to_file_path()
                .ok()
                .and_then(|path| params.config.get_project_for_path(&path));
            let effective_config = if let Some(project) = project_config
                && let Some(project_rules) = project.rules()
            {
                let merged_rules = params.config.rules().merge(project_rules);
                params.config.clone().with_rules(merged_rules)
            } else {
                params.config.clone()
            };

            let mut diagnostics = doc.get_semantic_diagnostics(
                &schema,
                filtered_fragments,
                Some(&used_fragments),
                Some(&effective_config),
                false,
                workspace_loaded,
            );

            // Add duplicate operation name diagnostics if enabled
            if params.config.rules().unique_operation_name()
                && let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = params.config.get_schema_for_path(&path)
            {
                add_duplicate_operation_diagnostics(
                    params.config,
                    &doc,
                    &uri,
                    &schema_key,
                    params.operation_names,
                    &mut diagnostics,
                );
            }

            (uri, doc.version, diagnostics)
        })
        .collect();

    // Publish and cache results sequentially (async)
    for (idx, (uri, version, diagnostics)) in results.into_iter().enumerate() {
        // Cache diagnostics for pull-based diagnostics
        if let Some(cache) = diagnostic_cache {
            cache.insert(
                uri.clone(),
                (version, params.result_id_epoch, diagnostics.clone()),
            );
        }

        if use_push {
            params
                .client
                .publish_diagnostics(uri, diagnostics, Some(version))
                .await;
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

    // End progress
    if let Some(p) = progress {
        p.end(Some(format!("Validated {} documents", total))).await;
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

/// Computes the set of URIs that need validation based on affected fragments and operations
#[allow(clippy::too_many_arguments)]
pub fn get_affected_uris(
    initial_uri: Url,
    affected_fragment_names: AHashSet<Arc<str>>,
    affected_spread_names: AHashSet<Arc<str>>,
    affected_operation_names: AHashSet<Arc<str>>,
    documents: &DocumentsMap,
    fragment_dependents: &FragmentDependentsMap,
    fragment_definitions: &FragmentDefinitionsMap,
    operation_names: &OperationNamesMap,
) -> Vec<Url> {
    let mut uris_to_validate = AHashSet::default();
    uris_to_validate.insert(initial_uri.clone());

    // Operation name changes can affect other files with the same name (duplicates)
    for op_name in affected_operation_names {
        if let Some(entry) = operation_names.get(&op_name) {
            for (_, uri) in entry.value() {
                uris_to_validate.insert(uri.clone());
            }
        }
    }

    let mut to_process: Vec<Arc<str>> = affected_fragment_names.into_iter().collect();
    let mut processed_fragments = AHashSet::default();

    while let Some(frag_name) = to_process.pop() {
        if !processed_fragments.insert(frag_name.clone()) {
            continue;
        }

        if let Some(dependents) = fragment_dependents.get(&frag_name) {
            for dep_uri in dependents.value().iter() {
                let dep_uri: &Url = dep_uri;
                if uris_to_validate.insert(dep_uri.clone())
                    && let Some(doc) = documents.get(dep_uri).map(|r| r.value().clone())
                {
                    for f in doc.fragments.iter() {
                        to_process.push(f.name.clone());
                    }
                }
            }
        }
    }

    for spread_name in affected_spread_names {
        if let Some(definitions) = fragment_definitions.get(&spread_name) {
            for def_uri in definitions.value().iter() {
                let def_uri: &Url = def_uri;
                uris_to_validate.insert(def_uri.clone());
            }
        }
    }

    uris_to_validate.into_iter().collect()
}

/// Gets all used fragments across the workspace
pub fn get_used_fragments(metadata: &MetadataMap) -> AHashSet<Arc<str>> {
    let mut used = AHashSet::default();
    for entry in metadata.iter() {
        for spread in entry.value().fragment_spreads.iter() {
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
    metadata: &MetadataMap,
    subgraphs: &Arc<DashMap<String, Vec<graphox_core::schema::SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
) -> Vec<FragmentCompletionInfo> {
    let all_fragments = super::fragment_manager::collect_fragment_metadata(
        metadata, config, subgraphs, documents, schemas,
    );

    get_fragments_for_doc_with_metadata(doc.package_root.as_deref(), &all_fragments)
}

/// Gets fragments available for a given document using pre-collected metadata
pub fn get_fragments_for_doc_with_metadata(
    target_package_root: Option<&std::path::Path>,
    all_fragments: &[FragmentCompletionInfo],
) -> Vec<FragmentCompletionInfo> {
    let mut filtered: Vec<_> = all_fragments
        .iter()
        .filter_map(|f| {
            let is_same_package =
                graphox_core::utils::paths_match(f.package_root.as_deref(), target_package_root);
            if is_same_package || f.is_public {
                Some((f, is_same_package))
            } else {
                None
            }
        })
        .collect();

    // Prioritize fragments from same package
    filtered.sort_by(|(a, a_same_pkg), (b, b_same_pkg)| {
        if a_same_pkg != b_same_pkg {
            return b_same_pkg.cmp(a_same_pkg);
        }

        b.is_public.cmp(&a.is_public).reverse()
    });

    filtered.into_iter().map(|(f, _)| f.clone()).collect()
}

/// Adds diagnostics for duplicate operation names within the same project
fn add_duplicate_operation_diagnostics(
    config: &graphox_core::Config,
    doc: &DocumentState,
    uri: &Url,
    schema_key: &str,
    operation_names: &OperationNamesMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check each operation in this document
    for op in doc.operations.iter() {
        if let Some(name) = &op.name {
            // Look up this operation name in the index
            if let Some(entry) = operation_names.get(name) {
                let path = uri.to_file_path().unwrap();
                let project_key = config
                    .get_project_for_path(&path)
                    .map(|p| p.include().as_key())
                    .unwrap_or_else(|| schema_key.to_string());
                // Filter to only operations in the same project (same schema)

                let locations_in_project: Vec<&Url> = entry
                    .value()
                    .iter()
                    .filter(|(p_key, _)| p_key.as_ref() == project_key)
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
                    let mut other_files: Vec<String> = locations_in_project
                        .iter()
                        .filter(|loc| **loc != uri)
                        .filter_map(|loc| loc.to_file_path().ok())
                        .map(|path| path.display().to_string())
                        .collect();

                    other_files.sort();
                    other_files.dedup();

                    if !other_files.is_empty() {
                        push_duplicate_operation_diagnostic(
                            diagnostics,
                            range,
                            name,
                            Some(other_files),
                        );
                    }

                    // Only report once per operation name in this file
                    break;
                }
            }
        }
    }
}
