use crate::backend::state::Backend;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

pub async fn handle_diagnostic(
    backend: &Backend,
    params: DocumentDiagnosticParams,
) -> Result<DocumentDiagnosticReportResult> {
    let start = std::time::Instant::now();

    // Get timeout duration
    let timeout_ms = {
        let config = backend.config.read().unwrap();
        config.get_timeouts().lsp_request_ms
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

        // Check if we have cached diagnostics
        if let Some(cached) = backend.diagnostic_cache.get(&uri) {
            let (cached_version, cached_diagnostics) = cached.value();

            // If the cached version matches the previous result ID, return unchanged
            if let Some(prev_result_id) = &params.previous_result_id
                && let Ok(prev_version) = prev_result_id.parse::<i32>()
                && prev_version == *cached_version
                && prev_version == doc_version
            {
                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
                        related_documents: None,
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id: cached_version.to_string(),
                        },
                    }),
                ));
            }

            // Return cached diagnostics if version matches
            if *cached_version == doc_version {
                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                        related_documents: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: Some(cached_version.to_string()),
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
            let (version, diagnostics) = cached.value();
            return Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(version.to_string()),
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
                    result_id: Some(doc_version.to_string()),
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
    let should_log = {
        let config = backend.config.read().unwrap();
        config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
    };

    if let Some((enabled, threshold_ms)) = should_log
        && enabled
    {
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
        config.get_timeouts().lsp_request_ms
    };

    // Apply timeout
    let res = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async move {
        let mut items = Vec::new();

        // Get all document URIs
        let all_uris: Vec<Url> = backend.documents.iter().map(|e| e.key().clone()).collect();

        // Validate all documents (this will cache diagnostics)
        backend.validate_all_documents().await;

        // Collect diagnostics from cache
        for uri in all_uris {
            if let Some(cached) = backend.diagnostic_cache.get(&uri) {
                let (version, diagnostics) = cached.value();

                // Check if this URI was in the previous result
                let unchanged = params
                    .previous_result_ids
                    .iter()
                    .any(|prev| prev.uri == uri && prev.value == version.to_string());

                if unchanged {
                    items.push(WorkspaceDocumentDiagnosticReport::Unchanged(
                        WorkspaceUnchangedDocumentDiagnosticReport {
                            uri: uri.clone(),
                            version: Some((*version) as i64),
                            unchanged_document_diagnostic_report:
                                UnchangedDocumentDiagnosticReport {
                                    result_id: version.to_string(),
                                },
                        },
                    ));
                } else {
                    items.push(WorkspaceDocumentDiagnosticReport::Full(
                        WorkspaceFullDocumentDiagnosticReport {
                            uri: uri.clone(),
                            version: Some((*version) as i64),
                            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                result_id: Some(version.to_string()),
                                items: diagnostics.clone(),
                            },
                        },
                    ));
                }
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
    let should_log = {
        let config = backend.config.read().unwrap();
        config.tracing.as_ref().map(|t| (t.enabled, t.threshold_ms))
    };

    if let Some((enabled, threshold_ms)) = should_log
        && enabled
    {
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
