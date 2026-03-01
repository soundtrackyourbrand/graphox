//! Codegen throttle module
//!
//! This module provides a throttling mechanism for automatic codegen runs
//! to prevent excessive codegen executions when many files change rapidly.

use std::collections::HashSet;
use std::sync::Weak;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, sleep};

use crate::backend::state::Backend;

/// A throttled codegen runner that debounces rapid codegen requests
pub struct CodegenThrottle {
    tx: mpsc::UnboundedSender<Option<String>>,
}

impl CodegenThrottle {
    /// Creates a new throttled codegen runner
    pub fn new(backend_weak: Weak<Backend>) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Option<String>>();

        // Spawn the throttle task
        tokio::spawn(async move {
            let mut last_run: Option<Instant> = None;

            loop {
                // Wait for a codegen request
                let first_project = match rx.recv().await {
                    Some(p) => p,
                    None => break, // Channel closed
                };

                let mut projects_to_run = HashSet::new();
                if let Some(p) = first_project {
                    projects_to_run.insert(p);
                }

                let (
                    throttle_ms,
                    config,
                    client,
                    type_caches,
                    documents,
                    metadata,
                    supports_progress,
                    position_encoding,
                ) = {
                    if let Some(backend) = backend_weak.upgrade() {
                        let cfg = backend.config.read().unwrap();
                        (
                            cfg.lsp_codegen_throttle_ms(),
                            cfg.clone(),
                            backend.client.clone(),
                            backend.type_caches.clone(),
                            backend.documents.clone(),
                            backend.metadata.clone(),
                            backend
                                .client_capabilities
                                .read()
                                .unwrap()
                                .supports_progress,
                            backend.get_position_encoding(),
                        )
                    } else {
                        break;
                    }
                };

                // Calculate time since last run
                let now = Instant::now();
                let time_since_last = last_run.map(|t| now.duration_since(t));

                // If we need to wait, sleep until throttle period has elapsed
                if let Some(time_since) = time_since_last {
                    let throttle_duration = Duration::from_millis(throttle_ms);
                    if time_since < throttle_duration {
                        let wait_time = throttle_duration - time_since;

                        // Drain any additional requests that come in during the wait period
                        tokio::select! {
                            _ = sleep(wait_time) => {
                                // Drain the channel of any requests that came in during sleep
                                let mut saw_none = projects_to_run.is_empty();
                                while let Ok(p) = rx.try_recv() {
                                    merge_project_msg(&mut saw_none, &mut projects_to_run, p);
                                }
                            }
                            res = rx.recv() => {
                                let mut saw_none = projects_to_run.is_empty();
                                if let Some(p) = res {
                                    merge_project_msg(&mut saw_none, &mut projects_to_run, p);
                                }
                                // Got another request during sleep, continue waiting
                                // and drain any other queued requests
                                while let Ok(p) = rx.try_recv() {
                                    merge_project_msg(&mut saw_none, &mut projects_to_run, p);
                                }
                                sleep(wait_time).await;
                            }
                        }
                    }
                }

                // Drain any remaining queued requests
                let mut saw_none = projects_to_run.is_empty();
                while let Ok(p) = rx.try_recv() {
                    merge_project_msg(&mut saw_none, &mut projects_to_run, p);
                }

                let final_projects = if projects_to_run.is_empty() {
                    None
                } else {
                    Some(projects_to_run)
                };

                // Run codegen
                super::codegen_runner::run_codegen(
                    client,
                    config,
                    type_caches,
                    documents,
                    metadata,
                    supports_progress,
                    final_projects,
                    position_encoding,
                )
                .await;

                last_run = Some(Instant::now());
            }
        });

        Self { tx }
    }

    /// Requests a codegen run (will be throttled)
    pub fn request_codegen(&self, project_key: Option<String>) {
        // Ignore send errors (would only happen if the receiver task has exited)
        let _ = self.tx.send(project_key);
    }
}

/// Helper function to merge a project message into the set of projects to run
fn merge_project_msg(
    saw_none: &mut bool,
    projects_to_run: &mut HashSet<String>,
    msg: Option<String>,
) {
    if let Some(p) = msg {
        if !*saw_none {
            projects_to_run.insert(p);
        }
    } else {
        *saw_none = true;
        projects_to_run.clear();
    }
}
