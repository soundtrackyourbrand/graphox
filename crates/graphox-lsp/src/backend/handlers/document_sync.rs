use crate::backend::helpers::{named_operation_names, update_operation_name_index};
use crate::backend::state::Backend;
use crate::backend::{document_changes, file_change_handler};
use graphox_core::DocumentState;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tower_lsp::lsp_types::*;

/// The distinct codegen-enabled project keys that own any of `uris`.
///
/// Codegen regenerates whole projects, so to keep a public fragment's
/// cross-project consumers up to date we trigger codegen for every project that
/// owns an affected document — its own project plus those of its transitive
/// consumers — rather than only the edited file's project. The set is
/// de-duplicated and projects with codegen disabled are skipped.
///
/// Callers must already have checked the master `lsp_automatic_codegen()` switch:
/// the LSP performs no automatic codegen when it is off.
fn codegen_project_keys<'a>(
    config: &graphox_core::Config,
    uris: impl Iterator<Item = &'a Url>,
) -> Vec<String> {
    let mut seen = ahash::AHashSet::default();
    let mut keys = Vec::new();
    for uri in uris {
        let Ok(path) = uri.to_file_path() else {
            continue;
        };
        let Some(project) = config.get_project_for_path(&path) else {
            continue;
        };
        let key = project.include().as_key();
        if !seen.insert(key.clone()) {
            continue;
        }
        if config.get_project_codegen_enabled(project) {
            keys.push(key);
        }
    }
    keys
}

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
        fragments: new_fragments.clone(),
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
    let old_operation_names: Option<Arc<[Arc<str>]>> = old_metadata
        .as_ref()
        .map(|m| named_operation_names(&m.operations));

    backend.update_dependency_indices(&uri, old_spreads.clone(), new_spreads.clone());
    backend.update_definition_indices(&uri, old_fragment_names.clone(), new_fragment_names.clone());

    // Update operation names index
    let config = backend.config.read().unwrap().clone();
    let affected_operation_names = update_operation_name_index(
        &backend.operation_names,
        &config,
        &uri,
        old_operation_names.as_deref(),
        &new_operations,
    );

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
    for name in new_fragment_names_set.intersection(&old_fragment_names_set) {
        if let Some(old_metadata) = &old_metadata
            && let Some(old_frag) = old_metadata.fragments.iter().find(|f| &f.name == name)
            && let Some(new_frag) = new_fragments.iter().find(|f| &f.name == name)
            && old_frag.source_hash != new_frag.source_hash
        {
            affected_fragment_names.insert(name.clone());
        }
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

    // Same gating as did_change: only churn the global caches when this open
    // actually changed a fragment definition / cross-document state.
    let fragments_changed = !affected_fragment_names.is_empty();
    let cross_document_changed = fragments_changed
        || !affected_spread_names.is_empty()
        || !affected_operation_names.is_empty();
    if fragments_changed {
        backend.invalidate_fragment_cache();
    }
    if cross_document_changed {
        backend.increment_workspace_version();
    }

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

    // The LSP performs no automatic codegen when `lsp_automatic_codegen` is off.
    let throttle = backend.codegen_throttle.read().unwrap().clone();
    if (had_graphql || has_graphql)
        && backend.workspace_loaded.load(Ordering::SeqCst)
        && config.lsp_automatic_codegen()
        && let Some(throttle) = throttle
    {
        for key in codegen_project_keys(&config, std::iter::once(&uri)) {
            throttle.request_codegen(Some(key));
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
        // Only invalidate the workspace-wide fragment metadata cache when a fragment
        // definition actually changed, and only bump the workspace epoch when the
        // edit can affect other documents. A pure operation-body/comment edit leaves
        // both untouched, so completion stays warm and the epoch doesn't churn.
        if result.fragments_changed {
            backend.invalidate_fragment_cache();
        }
        if result.cross_document_changed {
            backend.increment_workspace_version();
        }

        if !result.uris_to_validate.is_empty() {
            backend.validate_uris(result.uris_to_validate.clone()).await;
            backend.refresh_pull_diagnostics_for(&uri, &result.uris_to_validate);
        }

        // The LSP performs no automatic codegen when `lsp_automatic_codegen` is off.
        let throttle = backend.codegen_throttle.read().unwrap().clone();
        if backend.workspace_loaded.load(Ordering::SeqCst)
            && config.lsp_automatic_codegen()
            && let Some(throttle) = throttle
        {
            let has_graphql = backend
                .documents
                .get(&uri)
                .is_some_and(|d| !d.get_graphql_trees().is_empty());

            if has_graphql {
                // Regenerate every project that owns a document affected by this edit
                // (the validation closure) — not just the edited file's project — so a
                // public fragment's cross-project consumers don't keep stale generated
                // types.
                let keys = codegen_project_keys(
                    &config,
                    std::iter::once(&uri).chain(result.uris_to_validate.iter()),
                );
                for key in keys {
                    throttle.request_codegen(Some(key));
                }
            }
        }
    }
}

pub async fn handle_did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    let position_encoding = backend.get_position_encoding();

    // Re-validate on save just to be sure
    backend.validate_uris(vec![uri.clone()]).await;

    // Trigger codegen on save ONLY if the document contains GraphQL. Like every
    // LSP-driven codegen path, this is suppressed entirely when the master
    // `lsp_automatic_codegen` switch is off.
    let config = backend.config.read().unwrap().clone();
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

    let throttle = backend.codegen_throttle.read().unwrap().clone();
    if let Some(doc) = doc_for_codegen
        && !doc.get_graphql_trees().is_empty()
        && backend.workspace_loaded.load(Ordering::SeqCst)
        && config.lsp_automatic_codegen()
        && let Some(throttle) = throttle
    {
        // Regenerate the saved file's project plus any project that consumes a
        // fragment it defines (e.g. a public fragment used cross-project), so their
        // generated types are not left stale.
        let fragment_names: ahash::AHashSet<Arc<str>> =
            doc.fragments.iter().map(|f| f.name.clone()).collect();
        let affected = backend.get_affected_uris(
            uri.clone(),
            fragment_names,
            ahash::AHashSet::default(),
            ahash::AHashSet::default(),
        );
        let keys = codegen_project_keys(&config, std::iter::once(&uri).chain(affected.iter()));
        for key in keys {
            throttle.request_codegen(Some(key));
        }
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    let uri = backend.normalize_uri(params.text_document.uri);
    backend.open_documents.remove(&uri);

    let missing_on_disk = uri.to_file_path().ok().is_some_and(|path| !path.exists());
    let mut removed_from_workspace = false;

    if missing_on_disk {
        let config = backend.config.read().unwrap().clone();
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
            position_encoding: backend.get_position_encoding(),
        };

        if let Some(result) =
            file_change_handler::process_file_deleted(uri.clone(), &change_params, |uri| {
                backend.normalize_uri(uri)
            })
        {
            backend.invalidate_fragment_cache();
            backend.increment_workspace_version();
            removed_from_workspace = true;

            if result.should_reload_config {
                backend.reload_config().await;
            } else if !result.uris_to_validate.is_empty() {
                let uris_to_validate = result.uris_to_validate;
                backend.validate_uris(uris_to_validate.clone()).await;
                backend.refresh_pull_diagnostics_for(&uri, &uris_to_validate);
            }
        }
    }

    if !removed_from_workspace {
        // Clear from in-memory documents map to save memory.
        backend.documents.remove(&uri);
    }

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
                    backend.refresh_pull_diagnostics_for(&change_uri, &result.uris_to_validate);
                }

                let throttle = backend.codegen_throttle.read().unwrap().clone();
                // Request throttled codegen if enabled and workspace is loaded
                if result.should_run_codegen
                    && backend.workspace_loaded.load(Ordering::SeqCst)
                    && let Some(throttle) = throttle
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

#[cfg(test)]
mod tests {
    use super::*;
    use graphox_core::Config;
    use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};

    fn project(include: &str) -> ProjectConfig {
        ProjectConfig::default()
            .with_schema(SchemaSource::Single("schema.graphql".to_string()))
            .with_include(GlobPattern::Single(include.to_string()))
    }

    #[test]
    fn codegen_keys_cover_cross_project_consumers_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::write(base.join("schema.graphql"), "type Query { x: Int }").unwrap();
        for p in ["a", "b"] {
            std::fs::create_dir_all(base.join(p)).unwrap();
            std::fs::write(
                base.join(p).join("one.graphql"),
                "fragment F on Query { x }",
            )
            .unwrap();
            std::fs::write(base.join(p).join("two.graphql"), "query Q { x }").unwrap();
        }

        let config = Config::new_test(
            base.clone(),
            vec![project("a/**/*.graphql"), project("b/**/*.graphql")],
        );

        let a1 = Url::from_file_path(base.join("a/one.graphql")).unwrap();
        let a2 = Url::from_file_path(base.join("a/two.graphql")).unwrap();
        let b1 = Url::from_file_path(base.join("b/one.graphql")).unwrap();

        // A closure spanning both projects (public fragment in `a` consumed by `b`)
        // yields both project keys.
        let cross = codegen_project_keys(&config, [&a1, &a2, &b1].into_iter());
        assert_eq!(cross.len(), 2, "both projects covered, deduped: {cross:?}");

        // Multiple docs in the same project collapse to a single key.
        let same = codegen_project_keys(&config, [&a1, &a2].into_iter());
        assert_eq!(same.len(), 1, "same-project URIs dedupe: {same:?}");
    }
}
