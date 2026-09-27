//! A JSON-RPC client for the server's stdio transport.
//!
//! Server-to-client requests are answered the way VS Code answers them, since
//! the server's behaviour depends on those answers: watcher registrations are
//! recorded for [`crate::watcher`], and `workspace/diagnostic/refresh` is
//! forwarded to [`crate::pull_diagnostics`], which re-pulls in response.

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

type Callback = Box<dyn FnOnce(Result<Value, Value>) + Send>;

struct Pending {
    method: String,
    sent: Instant,
    callback: Callback,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Latency {
    pub count: u64,
    pub total: Duration,
    pub max: Duration,
}

/// Everything that crossed the wire, keyed by direction and method.
#[derive(Clone, Default, Debug)]
pub struct Traffic {
    /// `-> method` for messages sent to the server, `<- method` for messages
    /// received from it.
    pub counts: BTreeMap<String, u64>,
    /// Response times of client requests, by method.
    pub latency: BTreeMap<String, Latency>,
    /// Every response time in arrival order, so maxima can be taken per step.
    responses: Vec<(String, Duration)>,
    pub cancelled: u64,
    pub log_warnings: u64,
    pub log_errors: u64,
}

impl Traffic {
    fn bump(&mut self, key: String) {
        *self.counts.entry(key).or_default() += 1;
    }

    /// What happened since `earlier`, which must be an earlier snapshot of
    /// the same traffic.
    pub fn since(&self, earlier: &Traffic) -> Traffic {
        let responses =
            self.responses[earlier.responses.len().min(self.responses.len())..].to_vec();
        let mut maxima: HashMap<&str, Duration> = HashMap::new();
        for (method, elapsed) in &responses {
            let max = maxima.entry(method.as_str()).or_default();
            *max = (*max).max(*elapsed);
        }
        let counts = self
            .counts
            .iter()
            .filter_map(|(k, v)| {
                let d = v - earlier.counts.get(k).copied().unwrap_or(0);
                (d > 0).then(|| (k.clone(), d))
            })
            .collect();
        let latency = self
            .latency
            .iter()
            .filter_map(|(k, v)| {
                let before = earlier.latency.get(k).copied().unwrap_or_default();
                (v.count > before.count).then(|| {
                    (
                        k.clone(),
                        Latency {
                            count: v.count - before.count,
                            total: v.total - before.total,
                            max: maxima.get(k.as_str()).copied().unwrap_or_default(),
                        },
                    )
                })
            })
            .collect();
        Traffic {
            counts,
            latency,
            responses,
            cancelled: self.cancelled - earlier.cancelled,
            log_warnings: self.log_warnings - earlier.log_warnings,
            log_errors: self.log_errors - earlier.log_errors,
        }
    }

    pub fn count(&self, key: &str) -> u64 {
        self.counts.get(key).copied().unwrap_or(0)
    }
}

/// The session log: server log messages, stderr, watcher batches and step
/// markers, in one timeline.
pub struct Log {
    start: Instant,
    out: Mutex<BufWriter<File>>,
}

impl Log {
    pub fn create(path: &Path) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            start: Instant::now(),
            out: Mutex::new(BufWriter::new(File::create(path)?)),
        }))
    }

    pub fn line(&self, tag: &str, text: &str) {
        let t = self.start.elapsed().as_secs_f64();
        let mut out = self.out.lock().unwrap();
        for line in text.lines() {
            let _ = writeln!(out, "{t:>9.3} [{tag}] {line}");
        }
        let _ = out.flush();
    }
}

struct Shared {
    stdin: Mutex<ChildStdin>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Pending>>,
    traffic: Mutex<Traffic>,
    log: Arc<Log>,
    watchers: Mutex<(u64, Vec<Value>)>,
    active_progress: Mutex<HashSet<String>>,
    refresh_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl Shared {
    fn write(&self, message: &Value) {
        let body = message.to_string();
        let mut stdin = self.stdin.lock().unwrap();
        let _ = write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body);
        let _ = stdin.flush();
    }

    fn respond(&self, id: &Value, result: Value) {
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
}

pub struct LspClient {
    shared: Arc<Shared>,
    child: Mutex<Child>,
    pub pid: u32,
}

impl LspClient {
    pub fn spawn(server: &Path, cwd: &Path, log: Arc<Log>) -> io::Result<Arc<Self>> {
        let mut child = Command::new(server)
            .arg("lsp")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let shared = Arc::new(Shared {
            stdin: Mutex::new(child.stdin.take().unwrap()),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            traffic: Mutex::new(Traffic::default()),
            log: log.clone(),
            watchers: Mutex::new((0, Vec::new())),
            active_progress: Mutex::new(HashSet::new()),
            refresh_tx: Mutex::new(None),
        });

        let reader_shared = shared.clone();
        thread::spawn(move || read_loop(BufReader::new(stdout), reader_shared));
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log.line("stderr", &line);
            }
        });

        Ok(Arc::new(Self {
            shared,
            child: Mutex::new(child),
            pid,
        }))
    }

    /// Sends a request; `callback` runs on the reader thread with the result.
    pub fn request_with(
        &self,
        method: &str,
        params: Value,
        callback: impl FnOnce(Result<Value, Value>) + Send + 'static,
    ) -> i64 {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        self.shared.pending.lock().unwrap().insert(
            id,
            Pending {
                method: method.to_string(),
                sent: Instant::now(),
                callback: Box::new(callback),
            },
        );
        self.shared
            .traffic
            .lock()
            .unwrap()
            .bump(format!("-> {method}"));
        self.shared.write(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        id
    }

    /// Fire-and-forget request, like an editor feature whose answer nobody waits on.
    pub fn request_async(&self, method: &str, params: Value) -> i64 {
        self.request_with(method, params, |_| {})
    }

    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        let id = self.request_with(method, params, move |res| {
            let _ = tx.send(res);
        });
        match rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(format!("{method} failed: {e}")),
            Err(_) => {
                self.cancel(id);
                Err(format!("{method} timed out after {timeout:?}"))
            }
        }
    }

    pub fn cancel(&self, id: i64) {
        if self.shared.pending.lock().unwrap().contains_key(&id) {
            self.shared.traffic.lock().unwrap().cancelled += 1;
            self.notify("$/cancelRequest", json!({ "id": id }));
        }
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.shared
            .traffic
            .lock()
            .unwrap()
            .bump(format!("-> {method}"));
        self.shared
            .write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    pub fn traffic(&self) -> Traffic {
        self.shared.traffic.lock().unwrap().clone()
    }

    /// The registered file watchers, with a generation that changes on every
    /// registration.
    pub fn watchers(&self) -> (u64, Vec<Value>) {
        self.shared.watchers.lock().unwrap().clone()
    }

    pub fn progress_active(&self) -> bool {
        !self.shared.active_progress.lock().unwrap().is_empty()
    }

    pub fn on_diagnostic_refresh(&self, tx: mpsc::Sender<()>) {
        *self.shared.refresh_tx.lock().unwrap() = Some(tx);
    }

    pub fn is_running(&self) -> bool {
        matches!(self.child.lock().unwrap().try_wait(), Ok(None))
    }

    pub fn shutdown(&self, timeout: Duration) {
        let _ = self.request("shutdown", Value::Null, timeout);
        self.notify("exit", Value::Null);
        let deadline = Instant::now() + timeout;
        let mut child = self.child.lock().unwrap();
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap();
        if let Ok(None) = child.try_wait() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(v) = header.strip_prefix("Content-Length:") {
            length = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn read_loop(mut reader: impl BufRead, shared: Arc<Shared>) {
    while let Some(message) = read_message(&mut reader) {
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id");
        match (method, id) {
            (None, Some(id)) => {
                let Some(id) = id.as_i64() else { continue };
                let Some(pending) = shared.pending.lock().unwrap().remove(&id) else {
                    continue;
                };
                {
                    let mut traffic = shared.traffic.lock().unwrap();
                    let elapsed = pending.sent.elapsed();
                    let latency = traffic.latency.entry(pending.method.clone()).or_default();
                    latency.count += 1;
                    latency.total += elapsed;
                    latency.max = latency.max.max(elapsed);
                    traffic.responses.push((pending.method.clone(), elapsed));
                }
                let result = match message.get("error") {
                    Some(error) => Err(error.clone()),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                (pending.callback)(result);
            }
            (Some(method), Some(id)) => {
                shared.traffic.lock().unwrap().bump(format!("<- {method}"));
                handle_server_request(&shared, method, id, message.get("params"));
            }
            (Some(method), None) => {
                shared.traffic.lock().unwrap().bump(format!("<- {method}"));
                handle_notification(&shared, method, message.get("params"));
            }
            (None, None) => {}
        }
    }
    shared.log.line("client", "server closed stdout");
}

fn handle_server_request(shared: &Shared, method: &str, id: &Value, params: Option<&Value>) {
    match method {
        "client/registerCapability" => {
            let registrations = params
                .and_then(|p| p.get("registrations"))
                .and_then(Value::as_array);
            for registration in registrations.into_iter().flatten() {
                if registration.get("method").and_then(Value::as_str)
                    == Some("workspace/didChangeWatchedFiles")
                {
                    let watchers = registration
                        .pointer("/registerOptions/watchers")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let mut current = shared.watchers.lock().unwrap();
                    current.0 += 1;
                    current.1.extend(watchers);
                }
            }
            shared.respond(id, Value::Null);
        }
        "workspace/configuration" => {
            let items = params
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            shared.respond(id, Value::Array(vec![Value::Null; items]));
        }
        "workspace/diagnostic/refresh" => {
            shared.respond(id, Value::Null);
            if let Some(tx) = shared.refresh_tx.lock().unwrap().as_ref() {
                let _ = tx.send(());
            }
        }
        // The scenario never accepts edits the server initiates on its own.
        "workspace/applyEdit" => shared.respond(id, json!({ "applied": false })),
        _ => shared.respond(id, Value::Null),
    }
}

fn handle_notification(shared: &Shared, method: &str, params: Option<&Value>) {
    let Some(params) = params else { return };
    match method {
        "$/progress" => {
            let token = params
                .get("token")
                .map(Value::to_string)
                .unwrap_or_default();
            let kind = params.pointer("/value/kind").and_then(Value::as_str);
            let mut active = shared.active_progress.lock().unwrap();
            match kind {
                Some("begin") => {
                    active.insert(token);
                }
                Some("end") => {
                    active.remove(&token);
                }
                _ => {}
            }
        }
        "window/logMessage" | "window/showMessage" => {
            let level = params.get("type").and_then(Value::as_u64).unwrap_or(4);
            let text = params.get("message").and_then(Value::as_str).unwrap_or("");
            let tag = match level {
                1 => "error",
                2 => "warn",
                3 => "info",
                _ => "log",
            };
            {
                let mut traffic = shared.traffic.lock().unwrap();
                match level {
                    1 => traffic.log_errors += 1,
                    2 => traffic.log_warnings += 1,
                    _ => {}
                }
            }
            shared.log.line(tag, text);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_since_keeps_only_what_changed() {
        let mut before = Traffic::default();
        before.bump("-> a".into());
        before.latency.insert(
            "a".into(),
            Latency {
                count: 1,
                total: Duration::from_millis(50),
                max: Duration::from_millis(50),
            },
        );
        before
            .responses
            .push(("a".into(), Duration::from_millis(50)));
        let mut after = before.clone();
        after.bump("-> a".into());
        after.bump("<- b".into());
        after.latency.insert(
            "a".into(),
            Latency {
                count: 3,
                total: Duration::from_millis(80),
                max: Duration::from_millis(50),
            },
        );
        after
            .responses
            .push(("a".into(), Duration::from_millis(10)));
        after
            .responses
            .push(("a".into(), Duration::from_millis(20)));

        let diff = after.since(&before);
        assert_eq!(diff.count("-> a"), 1);
        assert_eq!(diff.count("<- b"), 1);
        assert_eq!(diff.latency["a"].count, 2);
        assert_eq!(diff.latency["a"].total, Duration::from_millis(30));
        // The earlier 50ms response belongs to the earlier step.
        assert_eq!(diff.latency["a"].max, Duration::from_millis(20));
    }
}
