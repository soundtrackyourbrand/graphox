use crate::backend::state::Backend;
use crate::backend::{document_changes, file_change_handler};
use ahash::AHashSet;
use graphox_core::DocumentState;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tower_lsp::lsp_types::*;

pub async fn handle_did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    if graphox_core::utils::has_generated_header(&params.text_document.text) {
        return;
    }
    let uri = backend.normalize_uri(params.text_document.uri.clone());
    let text = params.text_document.text;
    backend.open_documents.insert(uri.clone());
    let position_encoding = backend.get_position_encoding();

    let doc = DocumentState::new_from_thread_local(uri.clone(), &text, position_encoding.clone());
    let doc_arc = Arc::new(doc);

    // RECONCILIATION: Get old state before overwriting indices
    let old_fragments: Option<Vec<Arc<str>>> = backend
        .fragment_defs
        .get(&uri)
        .map(|f| f.iter().map(|f| f.name.clone()).collect());
    let old_spreads: Option<Vec<Arc<str>>> = backend
        .fragment_spreads
        .get(&uri)
        .map(|s| s.value().clone());
    let old_operations = backend.documents.get(&uri).map(|d| d.operations.clone());

    let mut affected_fragment_names = AHashSet::default();
    let mut affected_spread_names = AHashSet::default();
    let mut affected_operation_names = AHashSet::default();

    let new_fragment_names: Vec<Arc<str>> =
        doc_arc.fragments().iter().map(|f| f.name.clone()).collect();
    let new_spreads = doc_arc.fragment_spreads.clone();

    // Track changes to fragment definitions
    let old_fragment_names_set: HashSet<Arc<str>> = old_fragments
        .as_ref()
        .map(|f| f.iter().cloned().collect())
        .unwrap_or_default();
    let new_fragment_names_set: HashSet<Arc<str>> = new_fragment_names.iter().cloned().collect();

    for name in old_fragment_names_set.difference(&new_fragment_names_set) {
        affected_fragment_names.insert(name.clone());
    }
    for name in new_fragment_names_set.difference(&old_fragment_names_set) {
        affected_fragment_names.insert(name.clone());
    }

    // Track changes to fragment spreads
    let old_spreads_set: HashSet<Arc<str>> = old_spreads
        .as_ref()
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default();
    let new_spreads_set: HashSet<Arc<str>> = new_spreads.iter().cloned().collect();

    for name in old_spreads_set.difference(&new_spreads_set) {
        affected_spread_names.insert(name.clone());
    }
    for name in new_spreads_set.difference(&old_spreads_set) {
        affected_spread_names.insert(name.clone());
    }

    backend.documents.insert(uri.clone(), doc_arc.clone());

    // Update indices
    backend
        .fragment_defs
        .insert(uri.clone(), doc_arc.fragments().to_vec());
    backend
        .fragment_spreads
        .insert(uri.clone(), doc_arc.fragment_spreads.clone());
    backend
        .package_roots
        .insert(uri.clone(), doc_arc.package_root.clone());

    backend.update_dependency_indices(&uri, old_spreads.clone(), doc_arc.fragment_spreads.clone());
    backend.update_definition_indices(&uri, old_fragments.clone(), new_fragment_names);

    // Re-index operations for duplicate detection
    for mut entry in backend.operation_names.iter_mut() {
        let op_name = entry.key().clone();
        let mut removed = false;
        entry.value_mut().retain(|(_, op_uri)| {
            if op_uri == &uri {
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
    backend.operation_names.retain(|_, v| !v.is_empty());

    let config = backend.config.read().unwrap().clone();
    if let Ok(path) = uri.to_file_path()
        && let Some(schema_key) = config.get_schema_for_path(&path)
    {
        let project_key = config
            .get_project_for_path(&path)
            .map(|p| p.include().as_key())
            .unwrap_or_else(|| schema_key);
        let project_key_arc: Arc<str> = project_key.into();

        for op in doc_arc.operations() {
            if let Some(name) = &op.name {
                affected_operation_names.insert(name.clone());
                backend
                    .operation_names
                    .entry(name.clone())
                    .or_default()
                    .push((project_key_arc.clone(), uri.clone()));
            }
        }
    }

    // Invalidate fragment metadata cache
    backend.invalidate_fragment_cache();
    backend.increment_workspace_version();

    // Re-validate affected documents
    let affected_uris = backend.get_affected_uris(
        uri.clone(),
        affected_fragment_names,
        affected_spread_names,
        affected_operation_names,
    );
    backend.validate_uris(affected_uris).await;

    // Request codegen if enabled and document has/had GraphQL
    let had_graphql = old_fragments.as_ref().is_some_and(|f| !f.is_empty())
        || old_spreads.as_ref().is_some_and(|s| !s.is_empty())
        || old_operations.as_ref().is_some_and(|o| !o.is_empty());
    let has_graphql = !doc_arc.get_graphql_trees().is_empty();

    if had_graphql || has_graphql {
        if backend.workspace_loaded.load(Ordering::SeqCst) {
            if let Some(throttle) = &backend.codegen_throttle
                && let Ok(path) = uri.to_file_path()
            {
                let project_key = config
                    .get_project_for_path(&path)
                    .map(|p| p.include().as_key());
                throttle.request_codegen(project_key);
            }
        } else {
            backend
                .codegen_requested_during_scan
                .store(true, Ordering::SeqCst);
        }
    }
}

pub async fn handle_did_change(backend: &Backend, params: DidChangeTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    let version = params.text_document.version;
    let config = backend.config.read().unwrap().clone();
    let position_encoding = backend.get_position_encoding();

    // Process document changes and update indices
    let change_params = document_changes::DocumentChangeParams {
        documents: &backend.documents,
        fragment_defs: &backend.fragment_defs,
        fragment_spreads: &backend.fragment_spreads,
        package_roots: &backend.package_roots,
        fragment_dependents: &backend.fragment_dependents,
        fragment_definitions: &backend.fragment_definitions,
        operation_names: &backend.operation_names,
        config: &config,
        position_encoding,
    };

    if let Some(result) = document_changes::process_document_change(
        &uri,
        params.content_changes,
        version,
        &change_params,
    ) {
        // Invalidate fragment metadata cache since fragments might have changed
        backend.invalidate_fragment_cache();
        backend.increment_workspace_version();

        // Validate affected documents
        backend.validate_uris(result.uris_to_validate).await;
    }
}

pub async fn handle_did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri.clone());

    if let Ok(path) = uri.to_file_path() {
        let latest_text = params
            .text
            .clone()
            .or_else(|| backend.documents.get(&uri).map(|doc| doc.rope.to_string()));

        if let Some(latest_text) = latest_text {
            let should_sync_disk = std::fs::read_to_string(&path)
                .map(|disk_text| disk_text != latest_text)
                .unwrap_or(true);

            if should_sync_disk && let Err(e) = std::fs::write(&path, &latest_text) {
                crate::backend::error_logging::log_warning(
                    &backend.client,
                    "didSave",
                    format!(
                        "Failed to sync saved document to disk {}: {}",
                        path.display(),
                        e
                    ),
                )
                .await;
            }
        }
    }

    let config = backend.config.read().unwrap().clone();
    let position_encoding = backend.get_position_encoding();

    let change_params = file_change_handler::FileChangeParams {
        client: &backend.client,
        config: &config,
        documents: &backend.documents,
        fragment_defs: &backend.fragment_defs,
        fragment_spreads: &backend.fragment_spreads,
        package_roots: &backend.package_roots,
        fragment_dependents: &backend.fragment_dependents,
        fragment_definitions: &backend.fragment_definitions,
        operation_names: &backend.operation_names,
        gitignore: &backend.gitignore,
        position_encoding,
    };

    let result =
        file_change_handler::process_file_created_or_changed(uri.clone(), &change_params, |uri| {
            backend.normalize_uri(uri)
        })
        .await;

    if let Some(result) = result {
        backend.invalidate_fragment_cache();
        backend.increment_workspace_version();

        if !result.uris_to_validate.is_empty() {
            backend.validate_uris(result.uris_to_validate).await;
        }

        if result.should_run_codegen {
            if backend.workspace_loaded.load(Ordering::SeqCst) {
                if let Some(throttle) = &backend.codegen_throttle {
                    let project_key = if let Ok(path) = uri.to_file_path() {
                        backend
                            .config
                            .read()
                            .unwrap()
                            .get_project_for_path(&path)
                            .map(|p| p.include().as_key())
                    } else {
                        None
                    };
                    throttle.request_codegen(project_key);
                }
            } else {
                backend
                    .codegen_requested_during_scan
                    .store(true, Ordering::SeqCst);
            }
        }
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    backend.open_documents.remove(&uri);
    // We don't remove documents from the map because they might still be relevant
    // for other files (e.g. fragments). We only remove them if the file is deleted.
}

pub async fn handle_did_change_watched_files(
    backend: &Backend,
    params: DidChangeWatchedFilesParams,
) {
    let start = std::time::Instant::now();
    let timeout_ms = 10000;

    let _res = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let config = backend.config.read().unwrap().clone();
        let position_encoding = backend.get_position_encoding();

        for change in params.changes {
            let change_params = file_change_handler::FileChangeParams {
                client: &backend.client,
                config: &config,
                documents: &backend.documents,
                fragment_defs: &backend.fragment_defs,
                fragment_spreads: &backend.fragment_spreads,
                package_roots: &backend.package_roots,
                fragment_dependents: &backend.fragment_dependents,
                fragment_definitions: &backend.fragment_definitions,
                operation_names: &backend.operation_names,
                gitignore: &backend.gitignore,
                position_encoding: position_encoding.clone(),
            };

            let change_uri = change.uri.clone();
            let result =
                if change.typ == FileChangeType::CREATED || change.typ == FileChangeType::CHANGED {
                    file_change_handler::process_file_created_or_changed(
                        change.uri,
                        &change_params,
                        |uri| backend.normalize_uri(uri),
                    )
                    .await
                } else if change.typ == FileChangeType::DELETED {
                    file_change_handler::process_file_deleted(change.uri, &change_params, |uri| {
                        backend.normalize_uri(uri)
                    })
                } else {
                    None
                };

            if let Some(result) = result {
                // Invalidate fragment metadata cache since fragments might have changed
                backend.invalidate_fragment_cache();
                backend.increment_workspace_version();

                // Config reload takes precedence - if config changed, reload everything
                if result.should_reload_config {
                    backend.reload_config().await;
                    continue; // Skip other processing since we're doing a full reload
                }

                if result.should_reload_schema
                    && let Some(schema_path) = result.schema_path
                {
                    backend.reload_schema(&schema_path).await;
                }

                if !result.uris_to_validate.is_empty() {
                    backend.validate_uris(result.uris_to_validate.clone()).await;
                }

                // Request throttled codegen if enabled and workspace is loaded
                if result.should_run_codegen {
                    if backend.workspace_loaded.load(Ordering::SeqCst) {
                        if let Some(throttle) = &backend.codegen_throttle {
                            let project_key = if let Ok(path) = change_uri.to_file_path() {
                                backend
                                    .config
                                    .read()
                                    .unwrap()
                                    .get_project_for_path(&path)
                                    .map(|p| p.include().as_key())
                            } else {
                                None
                            };
                            throttle.request_codegen(project_key);
                        }
                    } else {
                        // Queue codegen for after workspace scan completes
                        backend
                            .codegen_requested_during_scan
                            .store(true, Ordering::SeqCst);
                    }
                }
            }
        }
    })
    .await;

    if _res.is_err() {
        let elapsed = start.elapsed();
        backend
            .client
            .log_message(
                tower_lsp::lsp_types::MessageType::ERROR,
                format!(
                    "LSP Request 'did_change_watched_files' exceeded timeout of {}ms (took {}ms)",
                    timeout_ms,
                    elapsed.as_millis()
                ),
            )
            .await;
    }

    // Extract tracing config
    let (enabled, threshold_ms) = {
        let config = backend.config.read().unwrap();
        let t = config.tracing();
        (t.enabled(), t.threshold_ms())
    };

    if enabled {
        let elapsed = start.elapsed();
        if elapsed.as_millis() >= threshold_ms as u128 {
            backend
                .client
                .log_message(
                    tower_lsp::lsp_types::MessageType::INFO,
                    format!(
                        "LSP Request 'did_change_watched_files' took {}ms",
                        elapsed.as_millis()
                    ),
                )
                .await;
        }
    }
}
