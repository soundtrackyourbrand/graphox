//! Delivers `workspace/didChangeWatchedFiles` from real file system events.
//!
//! Only paths matching the watchers the server registered are reported. That
//! filter is what decides whether the server hears about a change at all, and
//! whether its own codegen output feeds back into it.

use crate::lsp::{Log, LspClient};
use globset::{Glob, GlobBuilder, GlobMatcher};
use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

/// How string glob patterns (as opposed to relative patterns) are matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GlobBase {
    /// Against the absolute path, as VS Code matches them.
    Absolute,
    /// Against the path relative to the workspace root.
    Workspace,
}

const CREATE: u64 = 1;
const CHANGE: u64 = 2;
const DELETE: u64 = 4;

struct Rule {
    matcher: GlobMatcher,
    /// Set for relative patterns, which match paths relative to their base.
    base: Option<PathBuf>,
    kinds: u64,
}

pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    pub fn from_watchers(watchers: &[Value]) -> Self {
        let mut rules = Vec::new();
        for watcher in watchers {
            let kinds = watcher
                .get("kind")
                .and_then(Value::as_u64)
                .unwrap_or(CREATE | CHANGE | DELETE);
            let (pattern, base) = match watcher.get("globPattern") {
                Some(Value::String(p)) => (p.clone(), None),
                Some(rel) => {
                    let base = rel
                        .get("baseUri")
                        .and_then(|b| b.as_str().or_else(|| b.get("uri")?.as_str()))
                        .and_then(|u| u.strip_prefix("file://"))
                        .map(PathBuf::from);
                    let pattern = rel.get("pattern").and_then(Value::as_str);
                    match (pattern, base) {
                        (Some(p), Some(b)) => (p.to_string(), Some(b)),
                        _ => continue,
                    }
                }
                None => continue,
            };
            // VS Code globs never let `*` cross a path separator.
            match GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
                .map(|g: Glob| g.compile_matcher())
            {
                Ok(matcher) => rules.push(Rule {
                    matcher,
                    base,
                    kinds,
                }),
                Err(_) => continue,
            }
        }
        Self { rules }
    }

    pub fn matches(&self, path: &Path, root: &Path, glob_base: GlobBase, kind: u64) -> bool {
        self.rules.iter().any(|rule| {
            if rule.kinds & kind == 0 {
                return false;
            }
            match &rule.base {
                Some(base) => path
                    .strip_prefix(base)
                    .is_ok_and(|rel| rule.matcher.is_match(rel)),
                None => match glob_base {
                    GlobBase::Absolute => rule.matcher.is_match(path),
                    GlobBase::Workspace => path
                        .strip_prefix(root)
                        .is_ok_and(|rel| rule.matcher.is_match(rel)),
                },
            }
        })
    }

    /// Patterns that match none of `files`, i.e. registrations that never fire.
    pub fn dead_patterns(
        watchers: &[Value],
        files: &[PathBuf],
        root: &Path,
        glob_base: GlobBase,
    ) -> Vec<String> {
        watchers
            .iter()
            .filter(|w| {
                let rules = Rules::from_watchers(std::slice::from_ref(w));
                !files
                    .iter()
                    .any(|f| rules.matches(f, root, glob_base, CREATE | CHANGE | DELETE))
            })
            .map(|w| {
                w.get("globPattern")
                    .map(Value::to_string)
                    .unwrap_or_default()
            })
            .collect()
    }
}

#[derive(Default)]
pub struct WatchCounters {
    /// Events delivered to the server.
    pub sent: AtomicU64,
    /// Delivered events for files the config classifies as codegen output.
    pub sent_output: AtomicU64,
    /// Events dropped because no registered watcher matched them.
    pub unmatched: AtomicU64,
    pub notifications: AtomicU64,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct WatchCounts {
    pub sent: u64,
    pub sent_output: u64,
    pub unmatched: u64,
    pub notifications: u64,
}

impl WatchCounts {
    pub fn since(&self, earlier: &WatchCounts) -> WatchCounts {
        WatchCounts {
            sent: self.sent - earlier.sent,
            sent_output: self.sent_output - earlier.sent_output,
            unmatched: self.unmatched - earlier.unmatched,
            notifications: self.notifications - earlier.notifications,
        }
    }
}

pub struct FileWatcher {
    _watcher: notify::RecommendedWatcher,
    pending: Arc<AtomicUsize>,
    counters: Arc<WatchCounters>,
}

pub struct WatchOptions {
    pub root: PathBuf,
    pub batch: Duration,
    pub glob_base: GlobBase,
}

impl FileWatcher {
    pub fn start(
        client: Arc<LspClient>,
        options: WatchOptions,
        is_output: impl Fn(&Path) -> bool + Send + 'static,
        log: Arc<Log>,
    ) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<(PathBuf, EventKind)>();
        let pending = Arc::new(AtomicUsize::new(0));
        let counters = Arc::new(WatchCounters::default());

        let raw_pending = pending.clone();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let Ok(event) = res else { return };
                for path in event.paths {
                    raw_pending.fetch_add(1, Ordering::SeqCst);
                    let _ = tx.send((path, event.kind));
                }
            })?;
        watcher.watch(&options.root, RecursiveMode::Recursive)?;

        let thread_pending = pending.clone();
        let thread_counters = counters.clone();
        thread::spawn(move || {
            let git_dir = options.root.join(".git");
            let mut batch: HashMap<PathBuf, bool> = HashMap::new();
            let mut rules = (u64::MAX, Rules::from_watchers(&[]));
            let mut received = 0usize;
            loop {
                match rx.recv_timeout(options.batch) {
                    Ok((path, kind)) => {
                        received += 1;
                        let created = matches!(kind, EventKind::Create(_));
                        let entry = batch.entry(path).or_insert(false);
                        *entry |= created;
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) if batch.is_empty() => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }

                let (generation, watchers) = client.watchers();
                if generation != rules.0 {
                    rules = (generation, Rules::from_watchers(&watchers));
                }

                let mut changes = Vec::new();
                let mut lines = Vec::new();
                for (path, created) in batch.drain() {
                    if path.starts_with(&git_dir) || path.is_dir() {
                        continue;
                    }
                    // Event kinds from the OS are coalesced and unreliable, so the
                    // type is decided by what is on disk now.
                    let (typ, kind) = if !path.exists() {
                        (3, DELETE)
                    } else if created {
                        (1, CREATE)
                    } else {
                        (2, CHANGE)
                    };
                    if !rules
                        .1
                        .matches(&path, &options.root, options.glob_base, kind)
                    {
                        thread_counters.unmatched.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let output = is_output(&path);
                    if output {
                        thread_counters.sent_output.fetch_add(1, Ordering::Relaxed);
                    }
                    lines.push(format!(
                        "{} {}{}",
                        ["", "created", "changed", "deleted"][typ as usize],
                        path.strip_prefix(&options.root).unwrap_or(&path).display(),
                        if output { " (codegen output)" } else { "" }
                    ));
                    let uri = graphox_core::utils::path_to_uri(&path)
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| format!("file://{}", path.display()));
                    changes.push(json!({ "uri": uri, "type": typ }));
                }
                if !changes.is_empty() {
                    thread_counters
                        .sent
                        .fetch_add(changes.len() as u64, Ordering::Relaxed);
                    thread_counters
                        .notifications
                        .fetch_add(1, Ordering::Relaxed);
                    log.line(
                        "watch",
                        &format!("didChangeWatchedFiles: {} events", changes.len()),
                    );
                    lines.sort();
                    log.line("watch", &lines.join("\n"));
                    client.notify(
                        "workspace/didChangeWatchedFiles",
                        json!({ "changes": changes }),
                    );
                }
                thread_pending.fetch_sub(received, Ordering::SeqCst);
                received = 0;
            }
        });

        Ok(Self {
            _watcher: watcher,
            pending,
            counters,
        })
    }

    /// No raw events are waiting to be batched or delivered.
    pub fn is_idle(&self) -> bool {
        self.pending.load(Ordering::SeqCst) == 0
    }

    pub fn counts(&self) -> WatchCounts {
        WatchCounts {
            sent: self.counters.sent.load(Ordering::Relaxed),
            sent_output: self.counters.sent_output.load(Ordering::Relaxed),
            unmatched: self.counters.unmatched.load(Ordering::Relaxed),
            notifications: self.counters.notifications.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u64 = CREATE | CHANGE | DELETE;

    #[test]
    fn string_globs_follow_the_chosen_base() {
        let root = Path::new("/work/repo");
        let rules = Rules::from_watchers(&[
            json!({ "globPattern": "**/*.{graphql,ts}" }),
            json!({ "globPattern": "schema/schema.graphqls" }),
        ]);
        let ts = root.join("apps/web/query.ts");
        let schema = root.join("schema/schema.graphqls");

        assert!(rules.matches(&ts, root, GlobBase::Absolute, ALL));
        assert!(!rules.matches(&schema, root, GlobBase::Absolute, ALL));
        assert!(rules.matches(&schema, root, GlobBase::Workspace, ALL));
        assert!(!rules.matches(
            &root.join("apps/web/query.tsx"),
            root,
            GlobBase::Absolute,
            ALL
        ));
    }

    #[test]
    fn relative_patterns_match_under_their_base_only() {
        let root = Path::new("/work/repo");
        let rules = Rules::from_watchers(&[json!({
            "globPattern": { "baseUri": "file:///work/repo/schema", "pattern": "*.graphqls" }
        })]);
        assert!(rules.matches(
            &root.join("schema/a.graphqls"),
            root,
            GlobBase::Absolute,
            ALL
        ));
        assert!(!rules.matches(
            &root.join("other/a.graphqls"),
            root,
            GlobBase::Absolute,
            ALL
        ));
    }

    #[test]
    fn watch_kinds_filter_events() {
        let root = Path::new("/work/repo");
        let rules = Rules::from_watchers(&[json!({ "globPattern": "**/*.ts", "kind": DELETE })]);
        let path = root.join("a.ts");
        assert!(rules.matches(&path, root, GlobBase::Absolute, DELETE));
        assert!(!rules.matches(&path, root, GlobBase::Absolute, CHANGE));
    }
}
