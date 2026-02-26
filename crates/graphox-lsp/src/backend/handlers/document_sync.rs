use crate::backend::state::Backend;
use crate::backend::{document_changes, error_logging, file_change_handler};
use graphox_core::DocumentState;
use graphox_core::document::DocumentLanguage;

use ahash::AHashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tower_lsp::lsp_types::*;

pub async fn handle_did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    if graphox_core::utils::has_generated_header(&params.text_document.text) {
        return;
    }
    let uri = backend.normalize_uri(params.text_document.uri.clone());
    backend.open_documents.insert(uri.clone());
    let _language = DocumentLanguage::from_uri(&uri);
    let position_encoding = if let Ok(caps) = backend.client_capabilities.read() {
        caps.negotiated_encoding()
    } else {
        PositionEncodingKind::UTF16
    };

    let doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &params.text_document.text,
        position_encoding,
    );
    let should_run_codegen = !doc.get_graphql_trees().is_empty();

    let mut affected_fragment_names = AHashSet::default();
    let mut affected_operation_names = AHashSet::default();
    for f in doc.fragments() {
        affected_fragment_names.insert(f.name.clone());
    }
    for op in doc.operations() {
        if let Some(name) = &op.name {
            affected_operation_names.insert(name.clone());
        }
    }

    // Update performance indices
    backend.invalidate_fragment_cache();
    backend
        .fragment_defs
        .insert(uri.clone(), doc.fragments().to_vec());
    backend
        .fragment_spreads
        .insert(uri.clone(), doc.fragment_spreads.clone());
    backend
        .package_roots
        .insert(uri.clone(), doc.package_root.clone());
    backend.update_dependency_indices(&uri, None, doc.fragment_spreads.clone());
    backend.update_definition_indices(
        &uri,
        None,
        doc.fragments().iter().map(|f| f.name.clone()).collect(),
    );

    // Update operation names index for duplicate detection
    backend.clear_operation_names_for_uri(&uri);
    let config = backend.config.read().unwrap().clone();
    if let Ok(path) = uri.to_file_path()
        && let Some(schema_key) = config.get_schema_for_path(&path)
    {
        let project_key = config
            .get_project_for_path(&path)
            .map(|p| p.include().as_key())
            .unwrap_or_else(|| schema_key);
        let project_key_arc: Arc<str> = project_key.into();

        for op in doc.operations() {
            if let Some(name) = &op.name {
                backend
                    .operation_names
                    .entry(name.clone())
                    .or_default()
                    .push((project_key_arc.clone(), uri.clone()));
            }
        }
    }

    let mut affected_spread_names = AHashSet::default();
    for s in &doc.fragment_spreads {
        affected_spread_names.insert(s.clone());
    }

    backend.documents.insert(uri.clone(), Arc::new(doc));
    backend.increment_workspace_version();

    let uris_to_validate = backend.get_affected_uris(
        uri,
        affected_fragment_names,
        affected_spread_names,
        affected_operation_names,
    );
    backend.validate_uris(uris_to_validate).await;

    // Request throttled codegen if enabled
    if should_run_codegen && let Some(throttle) = &backend.codegen_throttle {
        throttle.request_codegen();
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    backend.open_documents.remove(&uri);
}

pub async fn handle_did_change(backend: &Backend, params: DidChangeTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri.clone());
    let version = params.text_document.version;

    let position_encoding = if let Ok(caps) = backend.client_capabilities.read() {
        caps.negotiated_encoding()
    } else {
        PositionEncodingKind::UTF16
    };

    let config = backend.config.read().unwrap().clone();

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
                error_logging::log_warning(
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

    let position_encoding = if let Ok(caps) = backend.client_capabilities.read() {
        caps.negotiated_encoding()
    } else {
        PositionEncodingKind::UTF16
    };

    let config = backend.config.read().unwrap().clone();
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

    let result = file_change_handler::process_file_created_or_changed(uri, &change_params, |uri| {
        backend.normalize_uri(uri)
    })
    .await;

    if let Some(result) = result {
        backend.invalidate_fragment_cache();
        backend.increment_workspace_version();

        if result.should_reload_config {
            backend.reload_config().await;
            return;
        }

        if result.should_reload_schema
            && let Some(schema_path) = result.schema_path
        {
            backend.reload_schema(&schema_path).await;
        }

        if !result.uris_to_validate.is_empty() {
            backend.validate_uris(result.uris_to_validate).await;
        }

        if result.should_run_codegen {
            if backend.workspace_loaded.load(Ordering::SeqCst) {
                if let Some(throttle) = &backend.codegen_throttle {
                    throttle.request_codegen();
                }
            } else {
                backend
                    .codegen_requested_during_scan
                    .store(true, Ordering::SeqCst);
            }
        }
    }
}

pub async fn handle_did_change_watched_files(
    backend: &Backend,
    params: DidChangeWatchedFilesParams,
) {
    let start = std::time::Instant::now();

    // Get timeout duration
    let timeout_ms = {
        let config = backend.config.read().unwrap();
        config.get_timeouts().lsp_request_ms()
    };

    // Apply timeout
    let _res = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async move {
        let position_encoding = if let Ok(caps) = backend.client_capabilities.read() {
            caps.negotiated_encoding()
        } else {
            PositionEncodingKind::UTF16
        };

        let config = backend.config.read().unwrap().clone();
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
                    backend.validate_uris(result.uris_to_validate).await;
                }

                // Request throttled codegen if enabled and workspace is loaded
                if result.should_run_codegen {
                    if backend.workspace_loaded.load(Ordering::SeqCst) {
                        if let Some(throttle) = &backend.codegen_throttle {
                            throttle.request_codegen();
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
                MessageType::ERROR,
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
                    MessageType::INFO,
                    format!(
                        "LSP Request 'did_change_watched_files' took {}ms",
                        elapsed.as_millis()
                    ),
                )
                .await;
        }
    }
}
