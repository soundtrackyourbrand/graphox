//! Progress reporting utilities for LSP operations
//!
//! This module provides helpers for reporting progress on long-running operations
//! like workspace scanning, validation, and codegen.

use std::sync::atomic::{AtomicU64, Ordering};
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

/// Global counter for generating unique progress tokens
static PROGRESS_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Progress reporter for a single operation
#[derive(Clone)]
pub struct ProgressReporter {
    client: Client,
    token: NumberOrString,
    supports_progress: bool,
    started: bool,
}

impl ProgressReporter {
    /// Create a new progress reporter
    pub async fn new(client: Client, title: impl Into<String>, supports_progress: bool) -> Self {
        let token = NumberOrString::Number(PROGRESS_COUNTER.fetch_add(1, Ordering::SeqCst) as i32);

        let mut reporter = Self {
            client,
            token,
            supports_progress,
            started: false,
        };

        if supports_progress {
            reporter.begin(title.into(), None).await;
        }

        reporter
    }

    /// Begin progress reporting
    async fn begin(&mut self, title: String, message: Option<String>) {
        if !self.supports_progress || self.started {
            return;
        }

        // Send begin notification
        let _ = self
            .client
            .send_notification::<notification::Progress>(ProgressParams {
                token: self.token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                    WorkDoneProgressBegin {
                        title,
                        cancellable: Some(false),
                        message,
                        percentage: None,
                    },
                )),
            })
            .await;

        self.started = true;
    }

    /// Report progress update
    pub async fn report(&self, message: impl Into<String>, percentage: Option<u32>) {
        if !self.supports_progress || !self.started {
            return;
        }

        self.client
            .send_notification::<notification::Progress>(ProgressParams {
                token: self.token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                    WorkDoneProgressReport {
                        cancellable: Some(false),
                        message: Some(message.into()),
                        percentage,
                    },
                )),
            })
            .await;
    }

    /// End progress reporting
    pub async fn end(&self, message: Option<String>) {
        if !self.supports_progress || !self.started {
            return;
        }

        self.client
            .send_notification::<notification::Progress>(ProgressParams {
                token: self.token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message,
                })),
            })
            .await;
    }
}

impl Drop for ProgressReporter {
    fn drop(&mut self) {
        // Note: We can't await in Drop, so we can't send the end notification here
        // Callers should explicitly call .end() before dropping
    }
}
