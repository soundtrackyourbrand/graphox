//! Workspace scanning and indexing operations
//!
//! This module handles the background workspace scan that occurs when the LSP
//! initializes. It parses all GraphQL files, indexes fragments, and validates
//! documents in parallel.

use ahash::AHashMap;
use ahash::AHashSet;
use apollo_compiler::Schema;
use dashmap::{DashMap, DashSet};
use graphox_core::Config;
use graphox_core::document::DocumentState;
use graphox_core::types::{
    DiagnosticCacheMap, DocumentsMap, FragmentDefinitionsMap, FragmentDependentsMap, MetadataMap,
    OperationNamesMap,
};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::diagnostics::DocumentDiagnostics;
use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Percentage of progress where validation starts
pub const VALIDATION_PROGRESS_START: u32 = 70;

/// Parameters for workspace scanning operation
pub struct WorkspaceScanParams {
    pub client: Client,
    pub config: Config,
    pub supports_pull_diagnostics: bool,
    pub documents: DocumentsMap,
    pub metadata: MetadataMap,
    pub fragment_dependents: FragmentDependentsMap,
    pub fragment_definitions: FragmentDefinitionsMap,
    pub operation_names: OperationNamesMap,
    pub workspace_loaded: Arc<AtomicBool>,
    /// Tracks if codegen was requested during the scan - triggers codegen after scan completes
    pub codegen_requested_during_scan: Arc<AtomicBool>,
    pub trigger_codegen_after_scan: Option<Arc<dyn Fn() + Send + Sync>>,
    pub empty_schema: Arc<Schema>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub subgraphs:
        Arc<DashMap<String, Vec<graphox_core::schema::SubgraphInfo>, ahash::RandomState>>,
    pub validated_schemas:
        Arc<DashMap<String, Arc<apollo_compiler::validation::Valid<Schema>>, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<AtomicBool>,
    pub codegen_throttle:
        Arc<std::sync::RwLock<Option<Arc<super::codegen_throttle::CodegenThrottle>>>>,
    pub supports_progress: bool,
    pub bypass_cache: bool,
    pub diagnostic_cache: DiagnosticCacheMap,
    pub fragment_metadata_cache: Arc<std::sync::RwLock<Option<Arc<Vec<FragmentCompletionInfo>>>>>,
    pub position_encoding: PositionEncodingKind,
    pub workspace_version: Arc<std::sync::atomic::AtomicUsize>,
    pub last_full_validation_version: Arc<std::sync::atomic::AtomicUsize>,
    pub open_documents: Arc<DashSet<Url, ahash::RandomState>>,
}

/// Spawns a background workspace scan task
///
/// This function extracts the large workspace scanning logic from Backend::initialized().
pub fn spawn_workspace_scan(params: WorkspaceScanParams) {
    tokio::spawn(async move {
        // Run the scan
        perform_workspace_scan(params).await;
    });
}

/// Performs the actual workspace scan
async fn perform_workspace_scan(params: WorkspaceScanParams) {
    let start_time = std::time::Instant::now();
    let root_dir = params.config.base_dir();

    // Create progress reporter
    let progress = super::progress::ProgressReporter::new(
        params.client.clone(),
        "Scanning workspace".to_string(),
        params.supports_progress,
    )
    .await;

    // First, identify all GraphQL files in the workspace (excluding ignored paths)
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root_dir)
        .hidden(false)
        .git_ignore(true)
        .build();

    // Collect files
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.file_type().is_some_and(|ft| ft.is_file())
            && graphox_core::utils::is_relevant_file(path)
        {
            files.push(path.to_path_buf());
        }
    }

    let total_files = files.len();
    progress
        .report(format!("Found {} files to scan", total_files), Some(10))
        .await;

    // Parallel scan: Parse all files and collect basic metadata
    let position_encoding = params.position_encoding.clone();
    let cancelled = params.workspace_scan_cancelled.clone();

    let scanned_docs: Vec<_> = files
        .into_par_iter()
        .filter_map(|path| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }

            let content = std::fs::read_to_string(&path).ok()?;
            if graphox_core::utils::has_generated_header(&content) {
                return None;
            }

            let uri = Url::from_file_path(&path).ok()?;
            let doc =
                DocumentState::new_from_thread_local(uri, &content, position_encoding.clone());

            if doc.get_graphql_trees().is_empty() {
                // If it doesn't contain GraphQL and it's not a direct .graphql file, skip it
                if path.extension().is_none_or(|ext| ext != "graphql") {
                    return None;
                }
            }

            Some(doc)
        })
        .collect();

    if cancelled.load(Ordering::Relaxed) {
        return;
    }

    // Update progress
    progress
        .report("Re-loading project schemas...".to_string(), Some(20))
        .await;

    // Pre-load project schemas BEFORE indexing documents to ensure they are available for validation
    for project in params.config.projects() {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        let key = project.schema().as_key();
        let use_cache = if params.bypass_cache {
            false
        } else {
            params.config.enable_schema_cache()
        };

        let schema_res =
            graphox_core::schema::load_schema_with_cache(root_dir, project.schema(), use_cache);

        match schema_res {
            Ok(schema) => {
                params.schemas.insert(key.clone(), Arc::new(schema.clone()));
                if let Ok(valid) = schema.validate() {
                    params.validated_schemas.insert(key, Arc::new(valid));
                }
            }
            Err(e) => {
                super::error_logging::log_error(
                    &params.client,
                    "Workspace Scanner",
                    format!("Failed to load schema for project {}: {}", key, e),
                )
                .await;
            }
        }
    }

    // Load subgraphs if any
    for project in params.config.projects() {
        if let Some(subgraphs_dir) = project.subgraphs_dir() {
            let subgraph_infos = graphox_core::schema::load_subgraphs(
                params.config.base_dir(),
                subgraphs_dir,
                project.subgraph_owners(),
            );
            if !subgraph_infos.is_empty() {
                params
                    .subgraphs
                    .insert(project.schema().as_key(), subgraph_infos);
            }
        }
    }

    // Index all documents sequentially (DashMap handles this well)
    for doc in scanned_docs {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        let uri = doc.uri.clone();

        let metadata = Arc::new(graphox_core::types::DocumentMetadata {
            fragments: doc.fragments.clone(),
            fragment_spreads: doc.fragment_spreads.clone(),
            package_root: doc.package_root.clone(),
            operations: doc.operations.clone(),
            version: doc.version,
        });
        // Only insert if the document is not already open in the editor
        if !params.open_documents.contains(&uri) {
            params.documents.insert(uri.clone(), Arc::new(doc.clone()));
            params.metadata.insert(uri.clone(), metadata);

            for frag in doc.fragments.iter() {
                params
                    .fragment_definitions
                    .entry(frag.name.clone())
                    .or_default()
                    .insert(uri.clone());
            }

            for spread in doc.fragment_spreads.iter() {
                params
                    .fragment_dependents
                    .entry(spread.clone())
                    .or_default()
                    .insert(uri.clone());
            }
        }
    }

    progress
        .report(
            "Validating workspace...".to_string(),
            Some(VALIDATION_PROGRESS_START),
        )
        .await;

    let validated_version = params.workspace_version.load(Ordering::SeqCst);

    // Run full workspace validation
    let (success, valid_empty_schema) =
        validate_all_documents_cancellable(&params, &progress, validated_version).await;
    if success {
        // Mark workspace as loaded
        params.workspace_loaded.store(true, Ordering::SeqCst);

        params
            .last_full_validation_version
            .store(validated_version, Ordering::SeqCst);

        if params.supports_pull_diagnostics {
            let open_uris: Vec<Url> = params
                .open_documents
                .iter()
                .map(|entry| entry.key().clone())
                .collect();
            if !open_uris.is_empty() {
                let validation_params = super::validation::ValidationParams {
                    client: &params.client,
                    documents: &params.documents,
                    config: &params.config,
                    metadata: &params.metadata,
                    validated_schemas: &params.validated_schemas,
                    valid_empty_schema: &valid_empty_schema,
                    workspace_loaded: &params.workspace_loaded,
                    open_documents: &params.open_documents,
                    fragment_dependents: &params.fragment_dependents,
                    fragment_definitions: &params.fragment_definitions,
                    operation_names: &params.operation_names,
                    subgraphs: &params.subgraphs,
                    schemas: &params.schemas,
                    supports_progress: false,
                    position_encoding: params.position_encoding.clone(),
                    result_id_epoch: validated_version,
                };
                super::validation::validate_uris(
                    validation_params,
                    open_uris,
                    false,
                    Some(&params.diagnostic_cache),
                )
                .await;
            }

            let client = params.client.clone();
            tokio::spawn(async move {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    client.workspace_diagnostic_refresh(),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        client
                            .log_message(
                                MessageType::WARNING,
                                format!(
                                    "workspace/diagnostic/refresh failed after workspace scan: {err}"
                                ),
                            )
                            .await;
                    }
                    Err(_) => {
                        client
                            .log_message(
                                MessageType::WARNING,
                                "workspace/diagnostic/refresh timed out after workspace scan",
                            )
                            .await;
                    }
                }
            });
        }

        let elapsed = start_time.elapsed();
        params
            .client
            .log_message(
                MessageType::INFO,
                format!(
                    "Workspace scan complete ({} documents) in {}ms",
                    params.documents.len(),
                    elapsed.as_millis()
                ),
            )
            .await;

        progress.end(None).await;

        // Trigger post-scan action if any
        if let Some(trigger) = params.trigger_codegen_after_scan {
            trigger();
        }
    } else {
        progress
            .end(Some("Workspace scan cancelled".to_string()))
            .await;
    }
}

/// Helper to validate all documents with cancellation support and progress reporting
async fn validate_all_documents_cancellable(
    params: &WorkspaceScanParams,
    progress: &super::progress::ProgressReporter,
    result_id_epoch: usize,
) -> (bool, Arc<apollo_compiler::validation::Valid<Schema>>) {
    let documents = &params.documents;
    let metadata = &params.metadata;
    let config = &params.config;
    let validated_schemas = &params.validated_schemas;
    let empty_schema = &params.empty_schema;
    let cancelled = &params.workspace_scan_cancelled;
    let client = &params.client;

    let valid_empty_schema = Arc::new(
        <apollo_compiler::Schema as Clone>::clone(empty_schema)
            .validate()
            .expect("Empty schema should always be valid"),
    );

    let used_fragments = super::validation::get_used_fragments(&params.metadata, config);
    let uris: Vec<Url> = documents
        .iter()
        .map(|e| e.key().clone())
        .filter(|uri| super::validation::is_configured_document_uri(uri, config))
        .collect();
    let total = uris.len();
    if total == 0 {
        return (true, valid_empty_schema);
    }

    // Pre-calculate all fragments info
    let all_fragments_info: Vec<(FragmentCompletionInfo, Option<Arc<str>>)> =
        super::fragment_manager::collect_fragment_metadata_with_schema(
            &params.metadata,
            config,
            &params.subgraphs,
            &params.documents,
            &params.schemas,
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
        if let Some(pkg_root) = &frag.package_root {
            fragments_by_package
                .entry(pkg_root.to_string_lossy().to_string())
                .or_default()
                .push(idx);
        }
    }

    // Validate in parallel batches to allow for progress updates and cancellation
    let batch_size = 50;
    let mut staged_diagnostics = Vec::new();

    for (batch_idx, batch) in uris.chunks(batch_size).enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return (false, valid_empty_schema);
        }

        let results: Vec<_> = batch
            .par_iter()
            .filter_map(|uri: &Url| {
                let doc = documents.get(uri)?;
                let meta = metadata.get(uri)?;

                let schema = if let Ok(path) = uri.to_file_path()
                    && let Some(schema_path) = config.get_schema_for_path(&path)
                    && let Some(schema) = validated_schemas.get(&schema_path)
                {
                    schema.value().clone()
                } else {
                    valid_empty_schema.clone()
                };

                // Filter fragments for this document
                let mut filtered_fragments = Vec::new();
                for &idx in &public_fragment_indices {
                    filtered_fragments.push(all_fragments_info[idx].0.clone());
                }

                if let Some(pkg_root) = &meta.package_root
                    && let Some(indices) =
                        fragments_by_package.get(&pkg_root.to_string_lossy().to_string())
                {
                    let mut seen: AHashSet<Arc<str>> = public_fragment_indices
                        .iter()
                        .map(|&i| all_fragments_info[i].0.name.clone())
                        .collect();
                    for &idx in indices {
                        if seen.insert(all_fragments_info[idx].0.name.clone()) {
                            filtered_fragments.push(all_fragments_info[idx].0.clone());
                        }
                    }
                }

                // Use project-specific rules if defined
                let project_config = uri
                    .to_file_path()
                    .ok()
                    .and_then(|path| config.get_project_for_path(&path));
                let effective_config = if let Some(project) = project_config {
                    let merged_rules = if let Some(project_rules) = project.rules() {
                        config.rules().merge(project_rules)
                    } else {
                        config.rules().clone()
                    };
                    config.clone().with_rules(merged_rules)
                } else {
                    config.clone()
                };

                let mut diagnostics = doc.get_semantic_diagnostics(
                    &schema,
                    &filtered_fragments,
                    Some(&used_fragments),
                    Some(&effective_config),
                    false, // verbose
                    false, // workspace_loaded (false because we are STILL loading)
                );

                // Duplicate operation name check
                if effective_config.rules().unique_operation_name() {
                    add_duplicate_operation_diagnostics(
                        &effective_config,
                        uri,
                        &doc,
                        &params.operation_names,
                        &mut diagnostics,
                    );
                }

                Some((uri.clone(), doc.version, diagnostics))
            })
            .collect();

        // Stage batch results
        staged_diagnostics.extend(results);

        // Update progress
        let current_count = ((batch_idx + 1) * batch_size).min(total);
        progress
            .report(
                format!("Validating documents ({}/{})", current_count, total),
                Some(
                    VALIDATION_PROGRESS_START
                        + ((current_count as f32 / total as f32)
                            * (100 - VALIDATION_PROGRESS_START) as f32)
                            as u32,
                ),
            )
            .await;
    }

    // Final cancellation check before committing staged diagnostics
    if cancelled.load(Ordering::Relaxed) {
        return (false, valid_empty_schema);
    }

    // Commit all staged diagnostics only if we finished without cancellation
    for (uri, version, diagnostics) in staged_diagnostics {
        params
            .diagnostic_cache
            .insert(uri.clone(), (version, result_id_epoch, diagnostics.clone()));
        if !params.supports_pull_diagnostics {
            client
                .publish_diagnostics(uri, diagnostics, Some(version))
                .await;
        }
    }

    (true, valid_empty_schema)
}

/// Adds diagnostics for duplicate operation names within the same project
fn add_duplicate_operation_diagnostics(
    config: &graphox_core::Config,
    uri: &Url,
    doc: &DocumentState,
    operation_names: &OperationNamesMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = match uri.to_file_path() {
        Ok(p) => p,
        Err(_) => return,
    };

    let schema_key = match config.get_schema_for_path(&path) {
        Some(k) => k,
        None => return,
    };

    let project_key = config
        .get_project_for_path(&path)
        .map(|p| p.include().as_key())
        .unwrap_or(schema_key);

    for op in doc.operations.iter() {
        if let Some(name) = &op.name
            && let Some(occurrences) = operation_names.get(name)
        {
            // Filter occurrences by the same project
            let other_files: Vec<String> = occurrences
                .value()
                .iter()
                .filter(|(proj, op_uri)| proj.as_ref() == project_key && op_uri != uri)
                .map(|(_, op_uri)| {
                    op_uri
                        .to_file_path()
                        .ok()
                        .map(|p| config.relativize(&p).to_string_lossy().to_string())
                        .unwrap_or_else(|| op_uri.to_string())
                })
                .collect();

            if !other_files.is_empty()
                && let Some(range) = graphox_core::utils::find_operation_range(doc, name)
            {
                graphox_core::utils::push_duplicate_operation_diagnostic(
                    diagnostics,
                    range,
                    name,
                    Some(other_files),
                );
            }
        }
    }
}
