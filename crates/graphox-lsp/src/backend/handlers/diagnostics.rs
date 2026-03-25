use crate::backend::state::Backend;
use ahash::AHashMap;
use std::sync::atomic::Ordering;
use tokio::time::{Duration, Instant};

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

fn compose_diagnostic_result_id(version: i32, workspace_epoch: usize) -> String {
    format!("{version}:{workspace_epoch}")
}

fn parse_diagnostic_result_id(result_id: &str) -> Option<(i32, usize)> {
    if let Some((version, workspace_epoch)) = result_id.split_once(':') {
        Some((version.parse().ok()?, workspace_epoch.parse().ok()?))
    } else {
        Some((result_id.parse().ok()?, 0))
    }
}

fn should_suppress_initial_required_field_diagnostics(
    backend: &Backend,
    diagnostics: &[Diagnostic],
) -> bool {
    !backend.workspace_loaded.load(Ordering::SeqCst)
        && diagnostics.iter().any(|diag| {
            matches!(
                diag.code.as_ref(),
                Some(NumberOrString::String(code)) if code == "required_field_missing"
            )
        })
}

pub async fn handle_diagnostic(
    backend: &Backend,
    params: DocumentDiagnosticParams,
) -> Result<DocumentDiagnosticReportResult> {
    let start = std::time::Instant::now();

    // Get timeout duration
    let timeout_ms = {
        let config = backend.config.read().unwrap();
        config.get_timeouts().lsp_request_ms()
    };

    // Apply timeout
    let res = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async move {
        let uri = backend.normalize_uri(params.text_document.uri.clone());

        // Get the current document version
        let doc_version = if let Some(doc) = backend.documents.get(&uri) {
            doc.version
        } else {
            // Document not found
            return Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: vec![],
                    },
                }),
            ));
        };

        if !backend.workspace_loaded.load(Ordering::SeqCst) {
            let wait_deadline =
                Instant::now() + Duration::from_millis(timeout_ms.min(200).saturating_sub(25));
            while Instant::now() < wait_deadline {
                if backend.workspace_loaded.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        let current_workspace_epoch = backend.workspace_version.load(Ordering::SeqCst);

        // Check if we have cached diagnostics
        if let Some(cached) = backend.diagnostic_cache.get(&uri) {
            let (cached_version, cached_workspace_epoch, cached_diagnostics) = cached.value();
            let cached_result_id =
                compose_diagnostic_result_id(*cached_version, *cached_workspace_epoch);
            let cache_is_current = *cached_version == doc_version
                && *cached_workspace_epoch == current_workspace_epoch;

            // If the cached version matches the previous result ID, return unchanged
            if let Some(prev_result_id) = &params.previous_result_id
                && let Some((prev_version, prev_workspace_epoch)) =
                    parse_diagnostic_result_id(prev_result_id)
                && cache_is_current
                && prev_version == *cached_version
                && prev_workspace_epoch == *cached_workspace_epoch
            {
                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
                        related_documents: None,
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id: cached_result_id,
                        },
                    }),
                ));
            }

            // Return cached diagnostics if version matches
            if cache_is_current {
                if should_suppress_initial_required_field_diagnostics(backend, cached_diagnostics) {
                    return Ok(DocumentDiagnosticReportResult::Report(
                        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                            related_documents: None,
                            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                result_id: Some(compose_diagnostic_result_id(doc_version, 0)),
                                items: vec![],
                            },
                        }),
                    ));
                }

                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                        related_documents: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: Some(cached_result_id),
                            items: cached_diagnostics.clone(),
                        },
                    }),
                ));
            }
        }

        // No cache or outdated cache - compute diagnostics
        // Force validation with caching but no push
        backend.validate_uris(vec![uri.clone()]).await;

        // Retrieve from cache
        if let Some(cached) = backend.diagnostic_cache.get(&uri) {
            let (version, workspace_epoch, diagnostics) = cached.value();
            if should_suppress_initial_required_field_diagnostics(backend, diagnostics) {
                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                        related_documents: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: Some(compose_diagnostic_result_id(doc_version, 0)),
                            items: vec![],
                        },
                    }),
                ));
            }

            return Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(compose_diagnostic_result_id(*version, *workspace_epoch)),
                        items: diagnostics.clone(),
                    },
                }),
            ));
        }

        // Fallback: return empty diagnostics
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: Some(compose_diagnostic_result_id(
                        doc_version,
                        current_workspace_epoch,
                    )),
                    items: vec![],
                },
            }),
        ))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let elapsed = start.elapsed();
            backend
                .client
                .log_message(
                    MessageType::ERROR,
                    format!(
                        "LSP Request 'diagnostic' exceeded timeout of {}ms (took {}ms) - returning empty response",
                        timeout_ms,
                        elapsed.as_millis()
                    ),
                )
                .await;
            // Return empty diagnostic report on timeout
            Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: vec![],
                    },
                }),
            ))
        }
    };

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
                    format!("LSP Request 'diagnostic' took {}ms", elapsed.as_millis()),
                )
                .await;
        }
    }
    res
}

pub async fn handle_workspace_diagnostic(
    backend: &Backend,
    params: WorkspaceDiagnosticParams,
) -> Result<WorkspaceDiagnosticReportResult> {
    let start = std::time::Instant::now();

    // Get timeout duration
    let timeout_ms = {
        let config = backend.config.read().unwrap();
        config.get_timeouts().lsp_request_ms()
    };

    // Apply timeout
    let res = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async move {
        if !backend.workspace_loaded.load(Ordering::SeqCst) {
            return Ok(WorkspaceDiagnosticReportResult::Report(
                WorkspaceDiagnosticReport { items: vec![] },
            ));
        }
        let current_workspace_epoch = backend.workspace_version.load(Ordering::SeqCst);
        let config = backend.config.read().unwrap().clone();

        // Get all document URIs
        let all_uris: Vec<Url> = backend
            .documents
            .iter()
            .map(|e| e.key().clone())
            .filter(|uri| crate::backend::validation::is_configured_document_uri(uri, &config))
            .collect();

        let uncached_uris: Vec<Url> = all_uris
            .iter()
            .filter(|uri| {
                let current_doc_version = backend.documents.get(*uri).map(|doc| doc.version);
                match (backend.diagnostic_cache.get(*uri), current_doc_version) {
                    (Some(cached), Some(doc_version)) => {
                        let (cached_version, cached_workspace_epoch, _) = cached.value();
                        *cached_version != doc_version
                            || *cached_workspace_epoch != current_workspace_epoch
                    }
                    _ => true,
                }
            })
            .cloned()
            .collect();

        if !uncached_uris.is_empty() {
            backend.validate_uris(uncached_uris).await;
        }

        // Convert previous_result_ids to a map for faster O(1) lookup
        let previous_ids: AHashMap<Url, String> = params
            .previous_result_ids
            .iter()
            .map(|prev| (prev.uri.clone(), prev.value.clone()))
            .collect();

        let mut items = Vec::new();

        // Collect diagnostics from cache
        for uri in all_uris {
            if let Some(cached) = backend.diagnostic_cache.get(&uri) {
                let (version, workspace_epoch, diagnostics) = cached.value();
                let result_id = compose_diagnostic_result_id(*version, *workspace_epoch);

                // Omitting unchanged entries keeps workspace polling cheap for clients like VS Code.
                if previous_ids
                    .get(&uri)
                    .is_some_and(|prev_val| prev_val == &result_id)
                {
                    continue;
                }

                items.push(WorkspaceDocumentDiagnosticReport::Full(
                    WorkspaceFullDocumentDiagnosticReport {
                        uri: uri.clone(),
                        version: Some((*version) as i64),
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: Some(result_id),
                            items: diagnostics.clone(),
                        },
                    },
                ));
            }
        }

        Ok(WorkspaceDiagnosticReportResult::Report(
            WorkspaceDiagnosticReport { items },
        ))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let elapsed = start.elapsed();
            backend
                .client
                .log_message(
                    MessageType::ERROR,
                    format!(
                        "LSP Request 'workspace_diagnostic' exceeded timeout of {}ms (took {}ms) - returning empty response",
                        timeout_ms,
                        elapsed.as_millis()
                    ),
                )
                .await;
            // Return empty workspace diagnostic report on timeout
            Ok(WorkspaceDiagnosticReportResult::Report(
                WorkspaceDiagnosticReport { items: vec![] },
            ))
        }
    };

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
                        "LSP Request 'workspace_diagnostic' took {}ms",
                        elapsed.as_millis()
                    ),
                )
                .await;
        }
    }
    res
}
