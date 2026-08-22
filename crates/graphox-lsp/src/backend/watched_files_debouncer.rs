//! Debouncer for `workspace/didChangeWatchedFiles` notifications.
//!
//! Non-editor file updates (a `git pull`, a branch switch, a rebase) arrive as a
//! burst of file-change events, often spread across several notifications. Handling
//! each change independently means one fragment-cache invalidation, one workspace
//! epoch bump, and one validation sweep *per file* — and because the validation
//! fragment-list cache is keyed on the workspace epoch, every per-file bump forces a
//! full rebuild of that list. On a large monorepo a branch switch could trigger
//! hundreds of those rebuilds.
//!
//! This debouncer coalesces a burst: it accumulates changes (collapsing repeated
//! events for the same file) and, once the changes go quiet for the configured
//! window, flushes the whole set to [`process_watched_file_batch`], which does the
//! expensive work exactly once over the union of affected documents.

use ahash::AHashMap;
use std::sync::Weak;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tower_lsp_server::ls_types::{FileChangeType, FileEvent, Uri};

use crate::backend::handlers::document_sync;
use crate::backend::state::Backend;

/// Debounces and batches watched-file change notifications.
pub struct WatchedFilesDebouncer {
    tx: mpsc::UnboundedSender<Vec<FileEvent>>,
}

impl WatchedFilesDebouncer {
    /// Spawns the background debounce task and returns a handle for submitting
    /// changes. The task lives as long as the channel is open and the `Backend`
    /// is alive.
    pub fn new(backend_weak: Weak<Backend>) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<FileEvent>>();

        tokio::spawn(async move {
            loop {
                // Block until the first batch of a new burst arrives.
                let first = match rx.recv().await {
                    Some(events) => events,
                    None => break, // channel closed
                };

                let mut pending: AHashMap<Uri, FileChangeType> = AHashMap::default();
                merge_events(&mut pending, first);

                // Reuse the same "debounce window for watched changes" knob as the
                // CLI codegen watcher; it is the natural setting for how long to wait
                // for a burst of file changes to settle.
                let debounce = match backend_weak.upgrade() {
                    Some(backend) => Duration::from_millis(
                        backend.config.read().unwrap().codegen_watch_debounce_ms(),
                    ),
                    None => break,
                };

                // Reset-timer debounce: extend the window each time more changes
                // arrive, so a sustained burst flushes only once it goes quiet.
                let channel_open = loop {
                    tokio::select! {
                        _ = sleep(debounce) => break true,
                        maybe = rx.recv() => match maybe {
                            Some(events) => merge_events(&mut pending, events),
                            None => break false, // channel closed; flush then exit
                        }
                    }
                };

                // Pull in anything already queued without waiting further.
                while let Ok(events) = rx.try_recv() {
                    merge_events(&mut pending, events);
                }

                let backend = match backend_weak.upgrade() {
                    Some(backend) => backend,
                    None => break,
                };

                let batch: Vec<FileEvent> = pending
                    .into_iter()
                    .map(|(uri, typ)| FileEvent { uri, typ })
                    .collect();
                document_sync::process_watched_file_batch(&backend, batch).await;

                if !channel_open {
                    break;
                }
            }
        });

        Self { tx }
    }

    /// Queues a notification's changes for batched processing.
    pub fn submit(&self, events: Vec<FileEvent>) {
        // Send errors only happen if the receiver task has exited, in which case
        // there is nothing useful to do.
        let _ = self.tx.send(events);
    }
}

/// Merges a notification's events into the pending set, collapsing repeated events
/// for the same file so it is processed once. The latest event type wins, which is
/// correct because create/change are processed identically and a final delete (or
/// re-create) reflects the file's end state.
fn merge_events(pending: &mut AHashMap<Uri, FileChangeType>, events: Vec<FileEvent>) {
    for event in events {
        pending.insert(event.uri, event.typ);
    }
}
