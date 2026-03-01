use crate::backend::state::Backend;
use crate::backend::{document_changes, error_logging, file_change_handler};
use graphox_core::DocumentState;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tower_lsp::lsp_types::*;

pub async fn handle_did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    if graphox_core::utils::has_generated_header(&params.text_document.text) {
        return;
    }
    let uri = backend.normalize_uri(params.text_document.uri);
    let text = params.text_document.text;
    let position_encoding = backend.get_position_encoding();

    backend.open_documents.insert(uri.clone());

    let doc = DocumentState::new_from_thread_local(uri.clone(), &text, position_encoding.clone());
    let doc_arc = Arc::new(doc);

    // Update indices and compute affected URIs
    let new_fragments = doc_arc.fragments.clone();
    let new_fragment_names: Arc<[Arc<str>]> =
        new_fragments.iter().map(|f| f.name.clone()).collect();
    let new_spreads = doc_arc.fragment_spreads.clone();
    let new_operations = doc_arc.operations.clone();

    let metadata = Arc::new(graphox_core::types::DocumentMetadata {
        fragments: new_fragments,
        fragment_spreads: new_spreads.clone(),
        package_root: doc_arc.package_root.clone(),
        operations: new_operations.clone(),
        version: params.text_document.version,
    });

    let old_metadata = backend.metadata.insert(uri.clone(), metadata);

    let old_spreads = old_metadata.as_ref().map(|m| m.fragment_spreads.clone());
    let old_fragment_names: Option<Arc<[Arc<str>]>> = old_metadata.as_ref().map(|m| {
        m.fragments
            .iter()
            .map(|f| f.name.clone())
            .collect::<Arc<[_]>>()
    });
    let old_operation_names: Option<Arc<[Arc<str>]>> = old_metadata.as_ref().map(|m| {
        m.operations
            .iter()
            .filter_map(|o| o.name.clone())
            .collect::<Arc<[_]>>()
    });

    backend.update_dependency_indices(&uri, old_spreads.clone(), new_spreads.clone());
    backend.update_definition_indices(&uri, old_fragment_names.clone(), new_fragment_names.clone());

    // Update operation names index
    let mut affected_operation_names = ahash::AHashSet::default();
    if let Ok(path) = uri.to_file_path()
        && let Some(schema_key) = backend.config.read().unwrap().get_schema_for_path(&path)
    {
        let project_key = backend
            .config
            .read()
            .unwrap()
            .get_project_for_path(&path)
            .map(|p| p.include().as_key())
            .unwrap_or_else(|| schema_key);
        let project_key_arc: Arc<str> = project_key.into();

        // Remove old entries
        if let Some(old) = old_operation_names {
            for name in old.iter() {
                affected_operation_names.insert(name.clone());
                if let Some(mut entry) = backend.operation_names.get_mut(name) {
                    entry.value_mut().retain(|(_, op_uri)| op_uri != &uri);
                }
            }
        }
        backend.operation_names.retain(|_, v| !v.is_empty());

        // Add new entries
        for op in new_operations.iter() {
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

    // Compute all affected fragment names using optimized HashSet comparison
    let mut affected_fragment_names = ahash::AHashSet::default();
    let old_fragment_names_set: HashSet<Arc<str>> = old_fragment_names
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

    // Compute all affected spread names using optimized HashSet comparison
    let mut affected_spread_names = ahash::AHashSet::default();
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

    // Invalidate fragment metadata cache
    backend.invalidate_fragment_cache();
    backend.increment_workspace_version();

    let uris_to_validate = crate::backend::validation::get_affected_uris(
        uri.clone(),
        affected_fragment_names,
        affected_spread_names,
        affected_operation_names,
        &backend.documents,
        &backend.fragment_dependents,
        &backend.fragment_definitions,
        &backend.operation_names,
    );

    // Initial validation
    backend.validate_uris(uris_to_validate).await;

    // Trigger initial codegen ONLY if the document contains GraphQL and automatic codegen is enabled
    // Or if it had GraphQL content before (to clean up)
    let had_graphql = old_metadata.as_ref().is_some_and(|m| {
        !m.fragments.is_empty() || !m.fragment_spreads.is_empty() || !m.operations.is_empty()
    });
    let has_graphql = !doc_arc.get_graphql_trees().is_empty();

    if (had_graphql || has_graphql)
        && backend.workspace_loaded.load(Ordering::SeqCst)
        && let Some(throttle) = &backend.codegen_throttle
    {
        let config = backend.config.read().unwrap().clone();
        let (is_enabled, project_key) = if let Ok(path) = uri.to_file_path() {
            let project = config.get_project_for_path(&path);
            let enabled = project
                .and_then(|p| p.codegen_enabled())
                .unwrap_or_else(|| config.lsp_automatic_codegen());
            (enabled, project.map(|p| p.include().as_key()))
        } else {
            (config.lsp_automatic_codegen(), None)
        };

        if is_enabled && config.lsp_automatic_codegen() {
            throttle.request_codegen(project_key);
        }
    }
}

pub async fn handle_did_change(backend: &Backend, params: DidChangeTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    let version = params.text_document.version;
    let changes = params.content_changes;
    let config = backend.config.read().unwrap().clone();
    let position_encoding = backend.get_position_encoding();

    // Process document changes and update indices
    let change_params = document_changes::DocumentChangeParams {
        documents: &backend.documents,
        metadata: &backend.metadata,
        fragment_dependents: &backend.fragment_dependents,
        fragment_definitions: &backend.fragment_definitions,
        operation_names: &backend.operation_names,
        config: &config,
        position_encoding,
    };

    if let Some(result) =
        document_changes::process_document_change(&uri, changes, version, &change_params)
    {
        // Invalidate fragment metadata cache since fragments might have changed
        backend.invalidate_fragment_cache();
        backend.increment_workspace_version();

        if !result.uris_to_validate.is_empty() {
            backend.validate_uris(result.uris_to_validate).await;
        }

        // Request throttled codegen ONLY if enabled and workspace is loaded
        // and only if the change result explicitly requested it.
        // NOTE: process_document_change is currently hardcoded to return false for didChange.
        if result.should_run_codegen
            && backend.workspace_loaded.load(Ordering::SeqCst)
            && let Some(throttle) = &backend.codegen_throttle
        {
            let (is_enabled, project_key) = if let Ok(path) = uri.to_file_path() {
                let project = config.get_project_for_path(&path);
                let enabled = project
                    .and_then(|p| p.codegen_enabled())
                    .unwrap_or_else(|| config.lsp_automatic_codegen());
                (enabled, project.map(|p| p.include().as_key()))
            } else {
                (config.lsp_automatic_codegen(), None)
            };

            if is_enabled {
                throttle.request_codegen(project_key);
            }
        }
    }
}

pub async fn handle_did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    let position_encoding = backend.get_position_encoding();

    // Persist latest in-memory content to disk before codegen
    if let Some(doc) = backend.documents.get(&uri)
        && let Ok(path) = uri.to_file_path()
    {
        let content = doc.rope.to_string();
        if let Err(e) = std::fs::write(&path, content) {
            error_logging::log_error(
                &backend.client,
                "Document Save",
                format!(
                    "Failed to write file to disk on save {}: {}",
                    path.display(),
                    e
                ),
            )
            .await;
        }
    }

    // Re-validate on save just to be sure
    backend.validate_uris(vec![uri.clone()]).await;

    // Trigger codegen on save (mandatory even if lsp_automatic_codegen is false)
    // ONLY if the document contains GraphQL
    let doc_for_codegen = if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone())
    {
        Some(doc)
    } else if let Ok(path) = uri.to_file_path()
        && let Ok(content) = std::fs::read_to_string(&path)
    {
        Some(Arc::new(DocumentState::new_from_thread_local(
            uri.clone(),
            &content,
            position_encoding,
        )))
    } else {
        None
    };

    if let Some(doc) = doc_for_codegen
        && !doc.get_graphql_trees().is_empty()
        && backend.workspace_loaded.load(Ordering::SeqCst)
        && let Some(throttle) = &backend.codegen_throttle
    {
        let config = backend.config.read().unwrap().clone();
        let project_key = if let Ok(path) = uri.to_file_path() {
            config
                .get_project_for_path(&path)
                .map(|p| p.include().as_key())
        } else {
            None
        };
        throttle.request_codegen(project_key);
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    backend.open_documents.remove(&uri);

    // Clear from in-memory documents map to save memory
    backend.documents.remove(&uri);

    // Clear diagnostics on client
    backend.client.publish_diagnostics(uri, vec![], None).await;
}

pub async fn handle_did_change_watched_files(
    backend: &Backend,
    params: DidChangeWatchedFilesParams,
) {
    let config = backend.config.read().unwrap().clone();
    let position_encoding = backend.get_position_encoding();

    for change in params.changes {
        let change_uri = change.uri.clone();
        if change.typ == FileChangeType::CREATED
            || change.typ == FileChangeType::CHANGED
            || change.typ == FileChangeType::DELETED
        {
            let change_params = file_change_handler::FileChangeParams {
                client: &backend.client,
                config: &config,
                documents: &backend.documents,
                metadata: &backend.metadata,
                fragment_dependents: &backend.fragment_dependents,
                fragment_definitions: &backend.fragment_definitions,
                operation_names: &backend.operation_names,
                gitignore: &backend.gitignore,
                diagnostic_cache: &backend.diagnostic_cache,
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
                    backend.validate_all_documents().await;
                    continue; // Skip other processing since we're doing a full reload
                }

                if result.should_reload_schema
                    && let Some(schema_path) = result.schema_path
                {
                    backend.reload_schema(&schema_path).await;
                    backend.validate_all_documents().await;
                }

                if !result.uris_to_validate.is_empty() {
                    backend.validate_uris(result.uris_to_validate.clone()).await;
                }

                // Request throttled codegen if enabled and workspace is loaded
                if result.should_run_codegen
                    && backend.workspace_loaded.load(Ordering::SeqCst)
                    && let Some(throttle) = &backend.codegen_throttle
                {
                    let (is_enabled, project_key) = if let Ok(path) = change_uri.to_file_path() {
                        let project = config.get_project_for_path(&path);
                        let enabled = project
                            .and_then(|p| p.codegen_enabled())
                            .unwrap_or_else(|| config.lsp_automatic_codegen());
                        (enabled, project.map(|p| p.include().as_key()))
                    } else {
                        (config.lsp_automatic_codegen(), None)
                    };

                    if is_enabled {
                        throttle.request_codegen(project_key);
                    }
                }
            }
        }
    }
}
