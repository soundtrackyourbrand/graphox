//! Workspace scanning and indexing operations
//!
//! This module handles the background workspace scan that occurs when the LSP
//! initializes. It parses all GraphQL files, indexes fragments, and validates
//! documents in parallel.

use ahash::AHashMap;
use ahash::AHashSet;
use apollo_compiler::Schema;
use dashmap::DashMap;
use graphql_core::Config;
use graphql_core::types::{
    DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap, FragmentDependentsMap,
    FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use graphql_features::completion::FragmentCompletionInfo;
use graphql_features::diagnostics::DocumentDiagnostics;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::Client;
use tower_lsp::lsp_types::*;
use tree_sitter::StreamingIterator;

/// Parameters for workspace scanning operation
pub struct WorkspaceScanParams {
    pub client: Client,
    pub config: Config,
    pub documents: DocumentsMap,
    pub fragment_defs: FragmentDefsMap,
    pub fragment_spreads: FragmentSpreadsMap,
    pub package_roots: PackageRootsMap,
    pub fragment_dependents: FragmentDependentsMap,
    pub fragment_definitions: FragmentDefinitionsMap,
    pub operation_names: OperationNamesMap,
    pub workspace_loaded: Arc<AtomicBool>,
    pub empty_schema: Arc<Schema>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<AtomicBool>,
    pub supports_progress: bool,
    pub fragment_metadata_cache: Arc<std::sync::RwLock<Option<Vec<FragmentCompletionInfo>>>>,
}

/// Spawns a background workspace scan task
///
/// This function extracts the large workspace scanning logic from Backend::initialized().
/// It runs in a separate tokio task to avoid blocking the LSP during initialization.
pub fn spawn_workspace_scan(params: WorkspaceScanParams) {
    tokio::spawn(async move {
        let timeout_ms = params.config.get_timeouts().workspace_scan_ms;
        let start = std::time::Instant::now();
        let client = params.client.clone();

        // Apply timeout to the entire workspace scan operation
        let scan_result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            perform_workspace_scan(params),
        )
        .await;

        match scan_result {
            Ok(()) => {
                let elapsed = start.elapsed();
                client
                    .log_message(
                        MessageType::INFO,
                        format!("Workspace scan complete in {}ms.", elapsed.as_millis()),
                    )
                    .await;
            }
            Err(_) => {
                let elapsed = start.elapsed();
                client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Workspace scan exceeded timeout of {}ms (took {}ms) and was aborted.",
                            timeout_ms,
                            elapsed.as_millis()
                        ),
                    )
                    .await;
            }
        }
    });
}

async fn perform_workspace_scan(params: WorkspaceScanParams) {
    // Create progress reporter
    let progress = super::progress::ProgressReporter::new(
        params.client.clone(),
        "Scanning workspace",
        params.supports_progress,
    )
    .await;

    let cancelled = params.workspace_scan_cancelled.clone();

    // Scan workspace and index all fragments/spreads
    progress
        .report("Discovering GraphQL files...", Some(10))
        .await;
    let workspace_metadata = scan_and_index_workspace(&params, &cancelled);

    // Invalidate fragment cache after indexing
    if let Ok(mut cache) = params.fragment_metadata_cache.write() {
        *cache = None;
    }

    let total_docs = workspace_metadata.documents.len();
    progress
        .report(
            format!("Indexed {} files, validating...", total_docs),
            Some(70),
        )
        .await;

    params.workspace_loaded.store(true, Ordering::SeqCst);

    // Validate all documents with proper schemas and fragments
    validate_all_documents(&params).await;

    // End progress
    progress
        .end(Some(format!("Finished scanning {} files", total_docs)))
        .await;
}

/// Scans workspace and indexes all fragments and spreads
fn scan_and_index_workspace(
    params: &WorkspaceScanParams,
    cancelled: &Arc<AtomicBool>,
) -> graphql_core::engine::WorkspaceMetadata {
    graphql_core::engine::Engine::scan_workspace_cancellable(
        &params.config,
        |_, doc| {
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            let uri = doc.uri.clone();

            params
                .fragment_defs
                .insert(uri.clone(), doc.fragments().to_vec());
            params
                .fragment_spreads
                .insert(uri.clone(), doc.fragment_spreads.clone());
            params
                .package_roots
                .insert(uri.clone(), doc.package_root.clone());

            for frag in doc.fragments() {
                params
                    .fragment_definitions
                    .entry(frag.name.clone())
                    .or_default()
                    .insert(uri.clone());
            }

            // Also index type definitions (for Go to Definition)
            let query = graphql_core::queries::GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, graphql_core::queries::GQL_DEFINITION_QUERY)
                    .expect("GQL_DEFINITION_QUERY should be a valid tree-sitter query")
            });
            let mut cursor = tree_sitter::QueryCursor::new();
            for block in doc.get_graphql_trees() {
                let mut matches =
                    cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                        doc.rope
                            .byte_slice(
                                (node.start_byte() + block.offset)
                                    ..(node.end_byte() + block.offset),
                            )
                            .chunks()
                    });
                while let Some(m) = matches.next() {
                    let name_node = m.captures[0].node;
                    let name = doc.get_node_text(name_node, block.offset);
                    params
                        .fragment_definitions
                        .entry(name.into())
                        .or_default()
                        .insert(uri.clone());
                }
            }

            for spread in &doc.fragment_spreads {
                params
                    .fragment_dependents
                    .entry(spread.clone())
                    .or_default()
                    .insert(uri.clone());
            }

            // Index operations for duplicate detection
            if let Ok(path) = uri.to_file_path()
                && let Some(schema_key) = params.config.get_schema_for_path(&path)
            {
                let schema_key_arc: Arc<str> = schema_key.into();
                for op in doc.operations() {
                    if let Some(name) = &op.name {
                        params
                            .operation_names
                            .entry(name.clone())
                            .or_default()
                            .push((schema_key_arc.clone(), uri.clone()));
                    }
                }
            }

            // If the document is not already open, we still might want to keep it in memory
            // for fast definition/hover/etc.
            if !params.documents.contains_key(&uri) {
                params.documents.insert(uri, Arc::new(doc));
            }
        },
        |_, _| {
            // Progress reporting is now handled by ProgressReporter in spawn_workspace_scan
        },
        cancelled.clone(),
    )
}

/// Validates all documents in the workspace
async fn validate_all_documents(params: &WorkspaceScanParams) {
    let documents = &params.documents;
    let config = &params.config;
    let fragment_defs = &params.fragment_defs;
    let fragment_spreads = &params.fragment_spreads;
    let package_roots = &params.package_roots;
    let schemas = &params.schemas;
    let empty_schema = &params.empty_schema;
    let client = &params.client;
    // Collect all used fragments
    let used_fragments = {
        let mut used = AHashSet::default();
        for entry in fragment_spreads.iter() {
            for spread in entry.value() {
                used.insert(spread.clone());
            }
        }
        used
    };

    // Pre-calculate validated schemas to avoid repeated validation
    let mut validated_schemas_map = AHashMap::default();
    for entry in schemas.iter() {
        let key = entry.key();
        match (**entry.value()).clone().validate() {
            Ok(valid) => {
                validated_schemas_map.insert(key.clone(), Arc::new(valid));
            }
            Err(e) => {
                client
                    .log_message(
                        MessageType::WARNING,
                        format!("Schema validation failed for {}: {}", key, e),
                    )
                    .await;
            }
        }
    }
    let valid_empty_schema = Arc::new(
        <apollo_compiler::Schema as Clone>::clone(empty_schema)
            .validate()
            .expect("Empty schema should always be valid"),
    );

    // Pre-calculate all fragments info
    let all_fragments_info: Vec<(FragmentCompletionInfo, Option<Arc<str>>)> =
        super::fragment_manager::collect_fragment_metadata_with_schema(
            fragment_defs,
            config,
            package_roots,
        );

    // Validate all documents in parallel
    use rayon::prelude::*;
    let to_publish: Vec<(Url, Vec<Diagnostic>)> = documents
        .as_ref()
        .par_iter()
        .map(|entry| {
            let uri = entry.key();
            let doc = entry.value();

            // Get schema for doc
            let (schema_key, schema): (
                Option<String>,
                Arc<apollo_compiler::validation::Valid<Schema>>,
            ) = if let Ok(path) = uri.to_file_path()
                && let Some(schema_path) = config.get_schema_for_path(&path)
                && let Some(schema) = validated_schemas_map.get(&schema_path)
            {
                (Some(schema_path), schema.clone())
            } else {
                (None, valid_empty_schema.clone())
            };

            // Filter fragments for this doc (same project/schema or public)
            let target_package_root = doc.package_root.as_ref();
            let filtered_fragments: Vec<FragmentCompletionInfo> = all_fragments_info
                .iter()
                .filter(|(f, f_schema_key)| {
                    let is_same_project = f_schema_key.as_ref().is_some_and(|k| {
                        schema_key.as_ref().is_some_and(|sk| k.as_ref() == sk)
                    });
                    let is_same_package = graphql_core::utils::paths_match(
                        f.package_root.as_deref(),
                        target_package_root.map(|p| p.as_path()),
                    );
                    is_same_project || is_same_package || f.is_public
                })
                .map(|(f, _)| f.clone())
                .collect();

            // If there are duplicate fragment names, prioritize the one in the same package,
            // then same project, then public.
            let mut sorted_fragments = filtered_fragments;
            sorted_fragments.sort_by(|a, b| {
                let a_same_pkg = graphql_core::utils::paths_match(
                    a.package_root.as_deref(),
                    target_package_root.map(|p| p.as_path()),
                );
                let b_same_pkg = graphql_core::utils::paths_match(
                    b.package_root.as_deref(),
                    target_package_root.map(|p| p.as_path()),
                );

                if a_same_pkg != b_same_pkg {
                    return b_same_pkg.cmp(&a_same_pkg);
                }

                // If both (or neither) are same package, prefer same project (already filtered)
                b.is_public.cmp(&a.is_public).reverse() // prefer non-public (local)
            });

            let diagnostics = doc.get_semantic_diagnostics(
                &schema,
                &sorted_fragments,
                Some(&used_fragments),
                Some(config),
                false,
                true,
            );
            (uri.clone(), diagnostics)
        })
        .collect();

    for (u, d) in to_publish {
        client.publish_diagnostics(u, d, None).await;
    }
}
