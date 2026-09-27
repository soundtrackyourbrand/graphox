//! Pull diagnostics the way VS Code's language client pulls them.
//!
//! The workspace report is re-requested a fixed interval after each response,
//! carrying the result ids of everything seen so far. A
//! `workspace/diagnostic/refresh` cancels any pull in flight, re-pulls every
//! open document, and restarts the workspace pull at once. That polling is
//! part of what an idle editor costs the server, so it runs for the whole
//! session rather than only around edits.

use crate::lsp::LspClient;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

enum Event {
    Refresh,
    WorkspaceDone(u64, Result<Value, Value>),
    Stop,
}

pub struct DiagnosticsPuller {
    tx: mpsc::Sender<Event>,
    doc_result_ids: Arc<Mutex<HashMap<String, String>>>,
    client: Arc<LspClient>,
}

impl DiagnosticsPuller {
    pub fn start(
        client: Arc<LspClient>,
        open_docs: Arc<Mutex<BTreeSet<String>>>,
        interval: Duration,
        workspace: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let doc_result_ids: Arc<Mutex<HashMap<String, String>>> = Default::default();

        let (refresh_tx, refresh_rx) = mpsc::channel();
        client.on_diagnostic_refresh(refresh_tx);
        let forward = tx.clone();
        thread::spawn(move || {
            while refresh_rx.recv().is_ok() {
                if forward.send(Event::Refresh).is_err() {
                    break;
                }
            }
        });

        let puller = Self {
            tx: tx.clone(),
            doc_result_ids: doc_result_ids.clone(),
            client: client.clone(),
        };

        thread::spawn(move || {
            let mut result_ids: HashMap<String, String> = HashMap::new();
            let mut in_flight: Option<(i64, u64)> = None;
            let mut generation = 0u64;
            let mut next_poll = workspace.then(Instant::now);

            loop {
                let wait = next_poll
                    .map(|t| t.saturating_duration_since(Instant::now()))
                    .unwrap_or(Duration::from_secs(3600));
                match rx.recv_timeout(wait) {
                    Ok(Event::Refresh) => {
                        pull_documents(&client, &open_docs.lock().unwrap(), &doc_result_ids);
                        if workspace {
                            if let Some((id, _)) = in_flight.take() {
                                client.cancel(id);
                            }
                            next_poll = Some(Instant::now());
                        }
                    }
                    Ok(Event::WorkspaceDone(done, result)) => {
                        if in_flight.is_some_and(|(_, g)| g == done) {
                            in_flight = None;
                            if let Ok(report) = result {
                                record_workspace_ids(&report, &mut result_ids);
                            }
                            next_poll = Some(Instant::now() + interval);
                        }
                    }
                    Ok(Event::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if in_flight.is_some() {
                            continue;
                        }
                        generation += 1;
                        let this = generation;
                        let previous: Vec<Value> = result_ids
                            .iter()
                            .map(|(uri, value)| json!({ "uri": uri, "value": value }))
                            .collect();
                        let done_tx = tx.clone();
                        let id = client.request_with(
                            "workspace/diagnostic",
                            json!({ "previousResultIds": previous }),
                            move |res| {
                                let _ = done_tx.send(Event::WorkspaceDone(this, res));
                            },
                        );
                        in_flight = Some((id, this));
                        next_poll = None;
                    }
                }
            }
        });

        puller
    }

    /// Pulls one document's diagnostics, as the editor does after a change.
    pub fn pull_document(&self, uri: &str) {
        pull_documents(
            &self.client,
            &BTreeSet::from([uri.to_string()]),
            &self.doc_result_ids,
        );
    }
}

impl Drop for DiagnosticsPuller {
    fn drop(&mut self) {
        let _ = self.tx.send(Event::Stop);
    }
}

fn pull_documents(
    client: &LspClient,
    uris: &BTreeSet<String>,
    result_ids: &Arc<Mutex<HashMap<String, String>>>,
) {
    for uri in uris {
        let previous = result_ids.lock().unwrap().get(uri).cloned();
        let ids = result_ids.clone();
        let key = uri.clone();
        client.request_with(
            "textDocument/diagnostic",
            json!({ "textDocument": { "uri": uri }, "previousResultId": previous }),
            move |res| {
                if let Some(id) = res.ok().and_then(|r| r.get("resultId").cloned())
                    && let Some(id) = id.as_str()
                {
                    ids.lock().unwrap().insert(key, id.to_string());
                }
            },
        );
    }
}

fn record_workspace_ids(report: &Value, ids: &mut HashMap<String, String>) {
    let Some(items) = report.get("items").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        if let (Some(uri), Some(id)) = (
            item.get("uri").and_then(Value::as_str),
            item.get("resultId").and_then(Value::as_str),
        ) {
            ids.insert(uri.to_string(), id.to_string());
        }
    }
}
