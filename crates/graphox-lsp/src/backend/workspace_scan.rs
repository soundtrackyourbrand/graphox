//! Workspace scanning and indexing operations
//!
//! This module handles the background workspace scan that occurs when the LSP
//! initializes. It parses all GraphQL files, indexes fragments, and validates
//! documents in parallel.

use ahash::AHashMap;
use ahash::AHashSet;
use apollo_compiler::Schema;
use dashmap::DashMap;
use graphox_core::Config;
use graphox_core::document::DocumentState;
use graphox_core::types::{
    DocumentsMap, FragmentDefinitionsMap, FragmentDefsMap, FragmentDependentsMap,
    FragmentSpreadsMap, OperationNamesMap, PackageRootsMap,
};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::diagnostics::DocumentDiagnostics;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::Client;
use tower_lsp::lsp_types::*;
use tree_sitter::StreamingIterator;

/// Percentage of progress where validation starts
const VALIDATION_PROGRESS_START: u32 = 70;

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
    /// Set to true if codegen was requested during the scan - triggers codegen after scan completes
    pub codegen_requested_during_scan: Arc<AtomicBool>,
    /// Callback to trigger codegen after scan completes (passed from Backend)
    pub trigger_codegen_after_scan: Option<Arc<dyn Fn() + Send + Sync>>,
    pub empty_schema: Arc<Schema>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub validated_schemas:
        Arc<DashMap<String, Arc<apollo_compiler::validation::Valid<Schema>>, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<AtomicBool>,
    pub codegen_throttle: Option<Arc<super::codegen_throttle::CodegenThrottle>>,
    pub supports_progress: bool,
    pub fragment_metadata_cache: Arc<std::sync::RwLock<Option<Vec<FragmentCompletionInfo>>>>,
    pub position_encoding: PositionEncodingKind,
}

/// Spawns a background workspace scan task
///
/// This function extracts the large workspace scanning logic from Backend::initialized().
/// It runs in a separate tokio task to avoid blocking the LSP during initialization.
pub fn spawn_workspace_scan(params: WorkspaceScanParams) {
    tokio::spawn(async move {
        let timeout_ms = params.config.get_timeouts().workspace_scan_ms();
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
            format!("Indexed {} files, Validating...", total_docs),
            Some(VALIDATION_PROGRESS_START),
        )
        .await;

    params.workspace_loaded.store(true, Ordering::SeqCst);

    // Trigger queued codegen if it was requested during the scan
    if params.codegen_requested_during_scan.load(Ordering::SeqCst) {
        params
            .codegen_requested_during_scan
            .store(false, Ordering::SeqCst);
        if let Some(throttle) = &params.codegen_throttle {
            throttle.request_codegen();
        }
    }

    // Validate all documents with proper schemas and fragments
    validate_all_documents_cancellable(&params, &cancelled, Some(&progress)).await;

    // End progress
    progress
        .end(Some(format!("Finished scanning {} files", total_docs)))
        .await;
}

/// Scans workspace and indexes all fragments and spreads
fn scan_and_index_workspace(
    params: &WorkspaceScanParams,
    cancelled: &Arc<AtomicBool>,
) -> graphox_core::engine::WorkspaceMetadata {
    graphox_core::engine::Engine::scan_workspace_cancellable(
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
            // Add timeout protection and max matches limit to prevent slow queries
            const MAX_QUERY_MATCHES: usize = 1000;
            const QUERY_TIMEOUT_MS: u64 = 100;

            let query = graphox_core::queries::GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, graphox_core::queries::GQL_DEFINITION_QUERY)
                    .expect("GQL_DEFINITION_QUERY should be a valid tree-sitter query")
            });
            let mut cursor = tree_sitter::QueryCursor::new();
            let query_start = std::time::Instant::now();

            for block in doc.get_graphql_trees() {
                // Check timeout
                if query_start.elapsed().as_millis() as u64 > QUERY_TIMEOUT_MS {
                    eprintln!(
                        "[graphox] Tree-sitter query timed out for URI {}, skipping remaining blocks",
                        uri
                    );
                    break;
                }

                let mut matches =
                    cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                        doc.rope
                            .byte_slice(
                                (node.start_byte() + block.offset)
                                    ..(node.end_byte() + block.offset),
                            )
                            .chunks()
                    });
                let mut match_count = 0;

                while let Some(m) = matches.next() {
                    // Check max matches limit
                    if match_count >= MAX_QUERY_MATCHES {
                        eprintln!(
                            "[graphox] Too many query matches for URI {}, stopping at {}",
                            uri, MAX_QUERY_MATCHES
                        );
                        break;
                    }
                    match_count += 1;

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
        params.position_encoding.clone(),
        None,
    )
}

/// Validates all documents in the workspace with cancellation support
async fn validate_all_documents_cancellable(
    params: &WorkspaceScanParams,
    cancelled: &Arc<AtomicBool>,
    progress: Option<&super::progress::ProgressReporter>,
) {
    let documents = &params.documents;
    let config = &params.config;
    let fragment_defs = &params.fragment_defs;
    let fragment_spreads = &params.fragment_spreads;
    let package_roots = &params.package_roots;
    let schemas = &params.schemas;
    let empty_schema = &params.empty_schema;
    let client = &params.client;

    // Check cancellation early
    if cancelled.load(Ordering::Relaxed) {
        eprintln!("[graphox] Validation cancelled early");
        return;
    }

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

    // PRE-INDEX FRAGMENTS BY PACKAGE AND SCHEMA (CRITICAL OPTIMIZATION)
    // This eliminates O(N×M) filtering during validation
    // Maps package_path_string -> list of fragment indices
    let mut fragments_by_package: AHashMap<String, Vec<usize>> = AHashMap::new();
    let mut public_fragment_indices: Vec<usize> = Vec::new();

    for (idx, (frag, _schema_key)) in all_fragments_info.iter().enumerate() {
        if frag.is_public {
            public_fragment_indices.push(idx);
        }
        let pkg_key = frag
            .package_root
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        fragments_by_package.entry(pkg_key).or_default().push(idx);
    }

    // Also build schema_key lookup
    let mut fragments_by_schema_key: AHashMap<Option<Arc<str>>, Vec<usize>> = AHashMap::new();
    for (idx, (_frag, schema_key)) in all_fragments_info.iter().enumerate() {
        fragments_by_schema_key
            .entry(schema_key.clone())
            .or_default()
            .push(idx);
    }

    // Validate all documents in parallel with cancellation support
    use rayon::prelude::*;
    use std::sync::atomic::AtomicUsize;

    // To avoid holding locks on the entire DashMap while validating in parallel,
    // we collect the documents and their URIs first.
    let docs_to_validate: Vec<(Url, Arc<DocumentState>)> = documents
        .iter()
        .map(|entry| (entry.key().clone(), entry.value().clone()))
        .collect();
    let total_docs = docs_to_validate.len();

    // Progress tracking using atomic counter
    let validated_count = Arc::new(AtomicUsize::new(0));
    let validated_count_clone = validated_count.clone();

    // Spawn a task to report progress from the parallel workers
    let progress_cloned: Option<super::progress::ProgressReporter> = progress.cloned();
    let validated_count_for_progress = validated_count_clone.clone();
    let progress_task = tokio::spawn(async move {
        let mut last_reported = 0;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let current = validated_count_for_progress.load(Ordering::Relaxed);
            if current >= total_docs {
                break;
            }
            if current > last_reported {
                let pct = VALIDATION_PROGRESS_START
                    + (current * (100 - VALIDATION_PROGRESS_START as usize)
                        / std::cmp::max(1, total_docs)) as u32;
                if let Some(ref p) = progress_cloned {
                    let _ = p
                        .report(
                            format!("Validating {}/{} documents", current, total_docs),
                            Some(pct),
                        )
                        .await;
                }
                last_reported = current;
            }
        }
    });

    let config_clone = config.clone();
    let used_fragments_clone = used_fragments.clone();
    let validated_schemas_map_clone = validated_schemas_map.clone();
    let valid_empty_schema_clone = valid_empty_schema.clone();
    let all_fragments_info_clone = all_fragments_info.clone();
    let fragments_by_package_clone = fragments_by_package.clone();
    let fragments_by_schema_key_clone = fragments_by_schema_key.clone();
    let public_fragment_indices_clone = public_fragment_indices.clone();
    let cancelled_clone = cancelled.clone();

    let to_publish: Vec<(Url, i32, Vec<Diagnostic>)> =
        match tokio::task::spawn_blocking(move || {
            docs_to_validate
                .into_par_iter()
                .enumerate()
                .map(|(idx, (uri, doc)): (usize, (Url, Arc<DocumentState>))| {
                    // Check cancellation periodically (every 100 documents)
                    if idx > 0 && idx % 100 == 0 && cancelled_clone.load(Ordering::Relaxed) {
                        return (uri, doc.version, Vec::new());
                    }

                    // Get schema for doc
                    let (schema_key, schema): (
                        Option<String>,
                        Arc<apollo_compiler::validation::Valid<Schema>>,
                    ) = if let Ok(path) = uri.to_file_path()
                        && let Some(schema_path) = config_clone.get_schema_for_path(&path)
                        && let Some(schema) = validated_schemas_map_clone.get(&schema_path)
                    {
                        (Some(schema_path), schema.clone())
                    } else {
                        (None, valid_empty_schema_clone.clone())
                    };

                    // FAST FRAGMENT LOOKUP using pre-built indices (O(1) instead of O(M))
                    // Collect relevant fragments: same package, same project, or public
                    let mut relevant_frags: Vec<usize> = Vec::with_capacity(64);
                    let doc_pkg_key = doc
                        .package_root
                        .as_ref()
                        .map(|p: &PathBuf| p.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // Same package
                    if let Some(pkg_frags) = fragments_by_package_clone.get(&doc_pkg_key) {
                        relevant_frags.extend(pkg_frags.iter().copied());
                    }

                    // Same project (different package but same schema_key)
                    if let Some(ref sk) = schema_key {
                        let sk_arc: Arc<str> = sk.as_str().into();
                        if let Some(project_frags) =
                            fragments_by_schema_key_clone.get(&Some(sk_arc))
                        {
                            for &idx in project_frags {
                                if !relevant_frags.contains(&idx) {
                                    relevant_frags.push(idx);
                                }
                            }
                        }
                    }

                    // Add public fragments (if not already included)
                    for &pub_idx in &public_fragment_indices_clone {
                        if !relevant_frags.contains(&pub_idx) {
                            relevant_frags.push(pub_idx);
                        }
                    }

                    // Clone only the fragments we actually need (reduced from M to ~few dozen)
                    let mut filtered_fragments: Vec<FragmentCompletionInfo> = relevant_frags
                        .iter()
                        .map(|&idx| all_fragments_info_clone[idx].0.clone())
                        .collect();

                    // If there are duplicate fragment names, prioritize the one in the same package,
                    // then same project, then public.
                    filtered_fragments.sort_by(|a, b| {
                        let a_same_pkg = graphox_core::utils::paths_match(
                            a.package_root.as_deref(),
                            doc.package_root.as_deref(),
                        );
                        let b_same_pkg = graphox_core::utils::paths_match(
                            b.package_root.as_deref(),
                            doc.package_root.as_deref(),
                        );

                        if a_same_pkg != b_same_pkg {
                            return b_same_pkg.cmp(&a_same_pkg);
                        }

                        b.is_public.cmp(&a.is_public).reverse()
                    });

                    let diagnostics = doc.get_semantic_diagnostics(
                        &schema,
                        &filtered_fragments,
                        Some(&used_fragments_clone),
                        Some(&config_clone),
                        false,
                        true,
                    );

                    // Increment progress
                    validated_count_clone.fetch_add(1, Ordering::Relaxed);

                    (uri, doc.version, diagnostics)
                })
                .collect()
        })
        .await
        {
            Ok(res) => res,
            Err(_) => {
                // Task was cancelled or panicked
                return;
            }
        };

    // Abort the progress task once validation is done
    progress_task.abort();

    for (u, v, d) in to_publish {
        client.publish_diagnostics(u, d, Some(v)).await;
    }
}
