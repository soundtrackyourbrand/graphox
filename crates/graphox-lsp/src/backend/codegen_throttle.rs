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
                    fragment_defs,
                    supports_progress,
                ) = {
                    if let Some(backend) = backend_weak.upgrade() {
                        let cfg = backend.config.read().unwrap();
                        (
                            cfg.lsp_codegen_throttle_ms(),
                            cfg.clone(),
                            backend.client.clone(),
                            backend.type_caches.clone(),
                            backend.documents.clone(),
                            backend.fragment_defs.clone(),
                            backend
                                .client_capabilities
                                .read()
                                .unwrap()
                                .supports_progress,
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
                                while let Ok(p) = rx.try_recv() {
                                    if let Some(p) = p {
                                        projects_to_run.insert(p);
                                    } else {
                                        // None means run all, so we can clear and stop accumulating
                                        projects_to_run.clear();
                                        break;
                                    }
                                }
                            }
                            res = rx.recv() => {
                                if let Some(p) = res {
                                    if let Some(p) = p {
                                        projects_to_run.insert(p);
                                    } else {
                                        projects_to_run.clear();
                                    }
                                }
                                // Got another request during sleep, continue waiting
                                // and drain any other queued requests
                                while let Ok(p) = rx.try_recv() {
                                    if let Some(p) = p {
                                        if !projects_to_run.is_empty() {
                                            projects_to_run.insert(p);
                                        }
                                    } else {
                                        projects_to_run.clear();
                                        break;
                                    }
                                }
                                sleep(wait_time).await;
                            }
                        }
                    }
                }

                // Drain any remaining queued requests
                while let Ok(p) = rx.try_recv() {
                    if let Some(p) = p {
                        if !projects_to_run.is_empty() {
                            projects_to_run.insert(p);
                        }
                    } else {
                        projects_to_run.clear();
                        break;
                    }
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
                    fragment_defs,
                    supports_progress,
                    final_projects,
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
