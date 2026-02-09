//! Codegen throttle module
//!
//! This module provides a throttling mechanism for automatic codegen runs
//! to prevent excessive codegen executions when many files change rapidly.

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, sleep};
use tower_lsp::Client;

use graphox_core::Config;

/// A throttled codegen runner that debounces rapid codegen requests
pub struct CodegenThrottle {
    tx: mpsc::UnboundedSender<()>,
}

impl CodegenThrottle {
    /// Creates a new throttled codegen runner
    pub fn new(
        client: Client,
        config: Arc<std::sync::RwLock<Config>>,
        type_caches: Arc<
            dashmap::DashMap<String, Arc<graphox_codegen::TypeCache>, ahash::RandomState>,
        >,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();

        // Spawn the throttle task
        tokio::spawn(async move {
            let mut last_run: Option<Instant> = None;

            loop {
                // Wait for a codegen request
                if rx.recv().await.is_none() {
                    // Channel closed, exit
                    break;
                }

                let throttle_ms = {
                    let cfg = config.read().unwrap();
                    cfg.lsp_codegen_throttle_ms()
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
                                while rx.try_recv().is_ok() {}
                            }
                            _ = rx.recv() => {
                                // Got another request during sleep, continue waiting
                                // and drain any other queued requests
                                while rx.try_recv().is_ok() {}
                                sleep(wait_time).await;
                            }
                        }
                    }
                }

                // Drain any remaining queued requests
                while rx.try_recv().is_ok() {}

                // Run codegen
                let client_clone = client.clone();
                let config_clone = config.read().unwrap().clone();
                let type_caches_clone = type_caches.clone();

                super::codegen_runner::run_codegen(
                    client_clone,
                    config_clone,
                    type_caches_clone,
                    false,
                )
                .await;

                last_run = Some(Instant::now());
            }
        });

        Self { tx }
    }

    /// Requests a codegen run (will be throttled)
    pub fn request_codegen(&self) {
        // Ignore send errors (would only happen if the receiver task has exited)
        let _ = self.tx.send(());
    }
}
