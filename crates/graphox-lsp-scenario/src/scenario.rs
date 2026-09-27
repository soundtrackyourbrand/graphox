//! A working session against one server, and the steps that make it up.
//!
//! Every step is measured the same way: from the start of its action until the
//! server has used less than a threshold of CPU for a quiet window, followed by
//! an idle window. A step that never quietens is the signature of sustained
//! load, so its stacks are sampled while it is still busy.

use crate::editor::Editor;
use crate::lsp::{Log, LspClient, Traffic};
use crate::process_stats::Sampler;
use crate::pull_diagnostics::DiagnosticsPuller;
use crate::watcher::{FileWatcher, GlobBase, Rules, WatchCounts, WatchOptions};
use crate::workspace::{self, Result, Site, Targets, git};
use graphox_core::Config;
use regex::Regex;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Step {
    OpenFiles,
    TypeOperation,
    TypeFragment,
    RenameFragment,
    MoveFile,
    ExternalEdits,
    BranchSwitch,
    Pull,
    Rebase,
    SchemaChange,
    Idle,
    Restart,
}

pub struct Options {
    pub server: PathBuf,
    pub repo: PathBuf,
    pub base: String,
    pub out_dir: PathBuf,
    pub quiet: Duration,
    pub threshold: f64,
    pub timeout: Duration,
    pub idle: Duration,
    pub idle_long: Duration,
    pub poll_interval: Duration,
    pub watch_batch: Duration,
    pub glob_base: GlobBase,
    pub keystroke: Duration,
    pub pull_commits: usize,
    pub branch: Option<String>,
    pub sample_secs: u64,
    pub sample_all: bool,
}

pub struct StepResult {
    pub name: String,
    pub iteration: usize,
    pub error: Option<String>,
    pub action: Duration,
    /// Time from the end of the action until the server went quiet.
    pub settle: Option<Duration>,
    pub unsettled: Option<&'static str>,
    pub cpu: Duration,
    pub peak_cores: f64,
    pub idle_cores: f64,
    pub rss_mb: f64,
    pub threads: u32,
    pub traffic: Traffic,
    pub watch: WatchCounts,
    pub sample: Option<PathBuf>,
}

struct Server {
    // Field order is drop order: the helpers go before the client they drive.
    editor: Editor,
    puller: DiagnosticsPuller,
    watcher: FileWatcher,
    sampler: Sampler,
    client: Arc<LspClient>,
}

pub struct Session {
    pub opts: Options,
    pub config: Config,
    pub targets: Targets,
    pub log: Arc<Log>,
    pub results: Vec<StepResult>,
    pub iteration: usize,
    server: Option<Server>,
}

const TYPED_FIELD: &str = "\n    __typename";

impl Session {
    pub fn new(opts: Options, config: Config, targets: Targets, log: Arc<Log>) -> Self {
        Self {
            opts,
            config,
            targets,
            log,
            results: Vec::new(),
            iteration: 0,
            server: None,
        }
    }

    fn server(&self) -> &Server {
        self.server.as_ref().expect("server not started")
    }

    fn editor(&mut self) -> &mut Editor {
        &mut self.server.as_mut().expect("server not started").editor
    }

    fn start_server(&mut self) -> Result<()> {
        let client = LspClient::spawn(&self.opts.server, &self.opts.repo, self.log.clone())
            .map_err(|e| format!("spawning {}: {e}", self.opts.server.display()))?;
        let sampler = Sampler::start(client.pid);
        let root_uri = crate::editor::uri_for(&self.opts.repo);
        let init = client.request(
            "initialize",
            initialize_params(&root_uri),
            Duration::from_secs(120),
        )?;
        client.notify("initialized", json!({}));
        let capabilities = init.get("capabilities").cloned().unwrap_or(Value::Null);
        let workspace_pull = capabilities
            .pointer("/diagnosticProvider/workspaceDiagnostics")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut editor = Editor::new(client.clone(), capabilities);
        editor.keystroke = self.opts.keystroke;
        let puller = DiagnosticsPuller::start(
            client.clone(),
            editor.open_uris.clone(),
            self.opts.poll_interval,
            workspace_pull,
        );
        let config = self.config.clone();
        let watcher = FileWatcher::start(
            client.clone(),
            WatchOptions {
                root: self.opts.repo.clone(),
                batch: self.opts.watch_batch,
                glob_base: self.opts.glob_base,
            },
            move |p| config.is_output_file(p),
            self.log.clone(),
        )
        .map_err(|e| format!("starting file watcher: {e}"))?;

        self.server = Some(Server {
            editor,
            puller,
            watcher,
            sampler,
            client,
        });
        Ok(())
    }

    fn stop_server(&mut self) {
        if let Some(server) = self.server.take() {
            server.client.shutdown(Duration::from_secs(10));
        }
    }

    /// Lists registered watchers that match no tracked file. Such a watcher
    /// never fires, so the server never hears about those files changing.
    pub fn report_dead_watchers(&self) {
        let (_, watchers) = self.server().client.watchers();
        if watchers.is_empty() {
            println!(
                "server registered no file watchers: it hears about files changing on disk \
                 only through open documents"
            );
            return;
        }
        let Ok(files) = git(&self.opts.repo, &["ls-files"]) else {
            return;
        };
        let files: Vec<PathBuf> = files.lines().map(|f| self.opts.repo.join(f)).collect();
        let dead = Rules::dead_patterns(&watchers, &files, &self.opts.repo, self.opts.glob_base);
        println!("server registered {} file watchers", watchers.len());
        for pattern in dead {
            println!(
                "  never fires ({:?} glob matching): {pattern}",
                self.opts.glob_base
            );
        }
    }

    fn measure(&mut self, name: &str, action: impl FnOnce(&mut Session) -> Result<()>) {
        self.measure_from(Instant::now(), name, action);
    }

    fn measure_from(
        &mut self,
        t0: Instant,
        name: &str,
        action: impl FnOnce(&mut Session) -> Result<()>,
    ) {
        self.log.line("step", &format!("==== {name} ===="));
        eprintln!("  {name}");
        let before = self
            .server
            .as_ref()
            .map(|s| (s.client.pid, s.client.traffic(), s.watcher.counts()));

        let profiler = self
            .opts
            .sample_all
            .then(|| self.spawn_sampler(name))
            .flatten();
        let error = action(self).err();
        let t_action = Instant::now();
        if let Some(e) = &error {
            self.log.line("step", &format!("failed: {e}"));
            eprintln!("    failed: {e}");
        }

        let Some(server) = self.server.as_ref() else {
            // A restart that failed to bring the server back still belongs in
            // the report, with its cause.
            if let Some((mut child, _)) = profiler {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.results.push(StepResult::failed(
                name,
                self.iteration,
                error.unwrap_or_else(|| "server not running after the action".to_string()),
            ));
            return;
        };
        // A restart replaces the server, whose counters then start from zero.
        let (traffic0, watch0) = match before {
            Some((pid, traffic, watch)) if pid == server.client.pid => (traffic, watch),
            _ => (Traffic::default(), WatchCounts::default()),
        };

        let (settled_at, unsettled) = self.wait_settled(t_action);
        let server = self.server();
        let busy_end = settled_at.unwrap_or_else(Instant::now);
        let sample = match profiler {
            Some((mut child, path)) => child.wait().ok().filter(|s| s.success()).map(|_| path),
            None => unsettled
                .filter(|_| server.client.is_running())
                .and_then(|_| self.sample_stacks(name)),
        };
        let server = self.server();

        let idle_start = Instant::now();
        thread::sleep(self.opts.idle);
        let idle_cores = server
            .sampler
            .cores_between(idle_start, Instant::now())
            .unwrap_or(0.0);
        let latest = server.sampler.latest();

        self.results.push(StepResult {
            name: name.to_string(),
            iteration: self.iteration,
            error,
            action: t_action - t0,
            settle: settled_at.map(|t| t.saturating_duration_since(t_action)),
            unsettled,
            cpu: server.sampler.cpu_between(t0, busy_end),
            peak_cores: server
                .sampler
                .peak_cores(t0, busy_end, Duration::from_secs(1)),
            idle_cores,
            rss_mb: latest.map_or(0.0, |s| s.snapshot.rss_bytes as f64 / 1_048_576.0),
            threads: latest.map_or(0, |s| s.snapshot.threads),
            traffic: server.client.traffic().since(&traffic0),
            watch: server.watcher.counts().since(&watch0),
            sample,
        });
    }

    /// Waits until the server has used under the threshold for a whole quiet
    /// window, with no watcher events in flight and no progress open. Returns
    /// the start of that window, or why it never came.
    fn wait_settled(&self, t_action: Instant) -> (Option<Instant>, Option<&'static str>) {
        let server = self.server();
        let deadline = t_action + self.opts.timeout;
        loop {
            thread::sleep(Duration::from_millis(250));
            let now = Instant::now();
            if !server.client.is_running() {
                return (None, Some("server exited"));
            }
            let quiet_start = now.checked_sub(self.opts.quiet).unwrap_or(t_action);
            let waiting_on = if quiet_start < t_action {
                Some("cpu")
            } else if !server.watcher.is_idle() {
                Some("watcher")
            } else if server.client.progress_active() {
                Some("progress")
            } else if server
                .sampler
                .cores_between(quiet_start, now)
                .is_none_or(|c| c >= self.opts.threshold)
            {
                Some("cpu")
            } else {
                None
            };
            match waiting_on {
                None => return (Some(quiet_start), None),
                Some(reason) if now >= deadline => return (None, Some(reason)),
                Some(_) => {}
            }
        }
    }

    fn sample_path(&self, step: &str) -> PathBuf {
        let slug: String = step
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        self.opts
            .out_dir
            .join(format!("{}-{slug}.sample.txt", self.iteration))
    }

    /// Starts sampling the server's stacks alongside a step's action.
    fn spawn_sampler(&self, step: &str) -> Option<(std::process::Child, PathBuf)> {
        let server = self.server.as_ref()?;
        if !cfg!(target_os = "macos") {
            return None;
        }
        let path = self.sample_path(step);
        let child = Command::new("sample")
            .arg(server.client.pid.to_string())
            .arg(self.opts.sample_secs.to_string())
            .arg("-file")
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        Some((child, path))
    }

    fn sample_stacks(&self, step: &str) -> Option<PathBuf> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let path = self.sample_path(step);
        eprintln!(
            "    still busy; sampling stacks for {}s",
            self.opts.sample_secs
        );
        let status = Command::new("sample")
            .arg(self.server().client.pid.to_string())
            .arg(self.opts.sample_secs.to_string())
            .arg("-file")
            .arg(&path)
            .output()
            .ok()?;
        status.status.success().then_some(path)
    }

    fn git(&mut self, args: &[&str]) -> Result<String> {
        let out = git(&self.opts.repo, args)?;
        self.editor().sync_from_disk();
        Ok(out)
    }

    pub fn startup(&mut self) {
        let t0 = Instant::now();
        match self.start_server() {
            Ok(()) => self.measure_from(t0, "startup (cold, no codegen output)", |_| Ok(())),
            Err(e) => {
                eprintln!("startup failed: {e}");
                self.results
                    .push(StepResult::failed("startup", self.iteration, e));
            }
        }
    }

    pub fn finish(&mut self) {
        self.stop_server();
    }

    pub fn run(&mut self, step: Step) {
        if self.server.as_ref().is_none_or(|s| !s.client.is_running()) {
            self.results.push(StepResult::failed(
                &format!("{step:?}"),
                self.iteration,
                "server is not running".to_string(),
            ));
            return;
        }
        match step {
            Step::OpenFiles => self.open_files(),
            Step::TypeOperation => {
                let site = self.targets.operation.clone();
                self.type_and_revert("operation", &site);
            }
            Step::TypeFragment => {
                let site = self.targets.fragment.clone();
                let label = format!(
                    "shared fragment ({} consumers)",
                    self.targets.fragment_consumers
                );
                self.type_and_revert(&label, &site);
            }
            Step::RenameFragment => self.rename_fragment(),
            Step::MoveFile => self.move_file(),
            Step::ExternalEdits => self.external_edits(),
            Step::BranchSwitch => self.branch_switch(),
            Step::Pull => self.pull(),
            Step::Rebase => self.rebase(),
            Step::SchemaChange => self.schema_change(),
            Step::Idle => {
                let secs = self.opts.idle_long;
                self.measure(&format!("idle {}s", secs.as_secs()), move |_| {
                    thread::sleep(secs);
                    Ok(())
                });
            }
            Step::Restart => self.measure("restart (warm)", |s| {
                let open = s.editor().open_paths();
                s.stop_server();
                s.start_server()?;
                for path in open {
                    s.editor().open(&path).map_err(|e| e.to_string())?;
                }
                Ok(())
            }),
        }
    }

    fn open_files(&mut self) {
        let files = self.targets.open_files.clone();
        let name = format!("open {} files", files.len());
        self.measure(&name, move |s| {
            let server = s.server.as_mut().unwrap();
            for path in &files {
                server.editor.open(path).map_err(|e| e.to_string())?;
                server.editor.pause(path, 0, &server.puller);
            }
            Ok(())
        });
    }

    fn check_site(&mut self, site: &Site) -> Result<()> {
        self.editor().open(&site.path).map_err(|e| e.to_string())?;
        let text = self.editor().text(&site.path).unwrap_or("");
        if text.as_bytes().get(site.insert_at.wrapping_sub(1)) != Some(&b'{') {
            return Err(format!(
                "{} no longer matches the base revision",
                site.path.display()
            ));
        }
        Ok(())
    }

    fn type_and_revert(&mut self, label: &str, site: &Site) {
        let rel = self.rel(&site.path);
        let site_typed = site.clone();
        self.measure(&format!("type into {label} {}", site.name), move |s| {
            s.check_site(&site_typed)?;
            let server = s.server.as_mut().unwrap();
            server.editor.type_text(
                &site_typed.path,
                site_typed.insert_at,
                TYPED_FIELD,
                &server.puller,
            );
            server.editor.navigate(&site_typed.path, site_typed.name_at);
            Ok(())
        });
        let path = site.path.clone();
        self.measure(&format!("save {rel}"), move |s| {
            s.editor().save(&path).map_err(|e| e.to_string())
        });
        let site_undo = site.clone();
        self.measure(&format!("undo and save {rel}"), move |s| {
            let editor = s.editor();
            let end = site_undo.insert_at + TYPED_FIELD.len();
            // Removing the range blind would delete real content if the typing
            // step failed before inserting anything.
            if editor
                .text(&site_undo.path)
                .and_then(|t| t.get(site_undo.insert_at..end))
                != Some(TYPED_FIELD)
            {
                return Err(format!(
                    "{} does not hold the typed field",
                    site_undo.path.display()
                ));
            }
            editor.change(
                &site_undo.path,
                vec![(site_undo.insert_at, end, String::new())],
            );
            editor.save(&site_undo.path).map_err(|e| e.to_string())
        });
    }

    fn rename_fragment(&mut self) {
        let site = self.targets.fragment.clone();
        let renamed = format!("{}Renamed", site.name);
        let (forward, back) = (site.clone(), site.clone());
        let new_name = renamed.clone();
        self.measure(
            &format!("rename fragment {} -> {renamed}", site.name),
            move |s| {
                s.check_site(&forward)?;
                let files = s
                    .editor()
                    .rename(&forward.path, forward.name_at, &new_name)?;
                s.log.line("step", &format!("rename touched {files} files"));
                Ok(())
            },
        );
        self.measure(
            &format!("rename fragment back to {}", site.name),
            move |s| {
                // The forward rename lengthens every spread above the definition
                // too, so the base revision's offset no longer points at it.
                let renamed = format!("{}Renamed", back.name);
                let pattern = Regex::new(&format!(r"\bfragment\s+({})\b", regex::escape(&renamed)))
                    .map_err(|e| e.to_string())?;
                let at = s
                    .editor()
                    .text(&back.path)
                    .and_then(|t| pattern.captures(t))
                    .and_then(|c| c.get(1))
                    .map(|m| m.start())
                    .ok_or_else(|| format!("{renamed} not found in {}", back.path.display()))?;
                let files = s.editor().rename(&back.path, at, &back.name)?;
                s.log.line("step", &format!("rename touched {files} files"));
                Ok(())
            },
        );
    }

    fn move_file(&mut self) {
        let from = self.targets.operation.path.clone();
        let stem = from.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = from.extension().and_then(|s| s.to_str()).unwrap_or("ts");
        let to = from.with_file_name(format!("{stem}-moved.{ext}"));
        let rel = self.rel(&from);
        let (a, b) = (from.clone(), to.clone());
        self.measure(&format!("move {rel} (open in editor)"), move |s| {
            rename_open(s, &a, &b)
        });
        self.measure("move it back", move |s| rename_open(s, &to, &from));
    }

    fn external_edits(&mut self) {
        let sites = self.targets.bulk_sites.clone();
        let n = sites.len();
        self.measure(&format!("edit {n} files outside the editor"), move |s| {
            for site in &sites {
                insert_on_disk(site, TYPED_FIELD)?;
            }
            s.editor().sync_from_disk();
            Ok(())
        });
        self.measure("git stash", |s| s.git(&["stash", "--quiet"]).map(drop));
        self.measure("git stash pop", |s| {
            s.git(&["stash", "pop", "--quiet"]).map(drop)
        });
        self.measure("discard with git checkout", |s| {
            s.git(&["checkout", "--quiet", "--", "."]).map(drop)
        });
    }

    fn branch_switch(&mut self) {
        let base = self.opts.base.clone();
        let picked = match &self.opts.branch {
            Some(b) => workspace::changed_files(&self.opts.repo, &base, b)
                .map(|c| Some((b.clone(), c.len()))),
            None => workspace::pick_branch(&self.opts.repo, &base, 30),
        };
        let (branch, changed) = match picked {
            Ok(Some(b)) => b,
            Ok(None) => {
                self.results.push(StepResult::failed(
                    "branch switch",
                    self.iteration,
                    "no other branch to switch to".into(),
                ));
                return;
            }
            Err(e) => {
                self.results
                    .push(StepResult::failed("branch switch", self.iteration, e));
                return;
            }
        };
        let target = branch.clone();
        self.measure(
            &format!("checkout {branch} ({changed} files differ)"),
            move |s| {
                s.git(&["checkout", "--quiet", "--detach", &target])
                    .map(drop)
            },
        );
        self.measure("checkout back to base", move |s| {
            s.git(&["checkout", "--quiet", "--detach", &base]).map(drop)
        });
    }

    fn pull(&mut self) {
        let base = self.opts.base.clone();
        let old = format!("{base}~{}", self.opts.pull_commits);
        let changed = workspace::changed_files(&self.opts.repo, &old, &base)
            .map(|c| c.len())
            .unwrap_or(0);
        let n = self.opts.pull_commits;
        let rewind = old.clone();
        self.measure(&format!("rewind {n} commits ({changed} files)"), move |s| {
            s.git(&["checkout", "--quiet", "--detach", &rewind])
                .map(drop)
        });
        self.measure(&format!("fast-forward {n} commits, as a pull"), move |s| {
            s.git(&["checkout", "--quiet", "--detach", &base]).map(drop)
        });
    }

    fn rebase(&mut self) {
        let base = self.opts.base.clone();
        let old = format!("{base}~{}", self.opts.pull_commits);
        let changed = match workspace::changed_files(&self.opts.repo, &old, &base) {
            Ok(c) => c,
            Err(e) => {
                self.results
                    .push(StepResult::failed("rebase", self.iteration, e));
                return;
            }
        };
        // Commits on files the upstream range also touches could conflict.
        let sites: Vec<Site> = self
            .targets
            .bulk_sites
            .iter()
            .filter(|s| !changed.contains(&self.rel(&s.path)))
            .take(3)
            .cloned()
            .collect();
        let n = self.opts.pull_commits;
        let start = old.clone();
        self.measure(&format!("branch off {n} commits back"), move |s| {
            s.git(&["checkout", "--quiet", "-B", "scenario/rebase", &start])
                .map(drop)
        });
        let count = sites.len();
        self.measure(&format!("commit {count} edits"), move |s| {
            for (i, site) in sites.iter().enumerate() {
                insert_on_disk(site, TYPED_FIELD)?;
                let rel = s.rel(&site.path);
                s.git(&["add", "--", &rel])?;
                s.git(&["commit", "--quiet", "-m", &format!("scenario edit {i}")])?;
            }
            Ok(())
        });
        let onto = base.clone();
        self.measure(&format!("rebase {count} commits onto base"), move |s| {
            s.git(&["rebase", "--quiet", &onto]).map(drop)
        });
        self.measure("return to base", move |s| {
            s.git(&["checkout", "--quiet", "--detach", &base])?;
            s.git(&["branch", "-D", "scenario/rebase"]).map(drop)
        });
    }

    fn schema_change(&mut self) {
        let Some(schema) = self.targets.schema_file.clone() else {
            self.results.push(StepResult::failed(
                "schema change",
                self.iteration,
                "no schema file in the config".into(),
            ));
            return;
        };
        let rel = self.rel(&schema);
        let path = schema.clone();
        self.measure(&format!("type a new type into {rel} and save"), move |s| {
            let server = s.server.as_mut().unwrap();
            server.editor.open(&path).map_err(|e| e.to_string())?;
            let end = server.editor.text(&path).map_or(0, str::len);
            server.editor.type_text(
                &path,
                end,
                "\ntype GraphoxScenario {\n  id: ID!\n}\n",
                &server.puller,
            );
            server.editor.save(&path).map_err(|e| e.to_string())
        });
        let path = schema.clone();
        let rel_revert = rel.clone();
        self.measure(&format!("revert {rel} with git checkout"), move |s| {
            s.git(&["checkout", "--quiet", "--", &rel_revert])?;
            s.editor().close(&path);
            Ok(())
        });
    }

    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.opts.repo)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }
}

impl StepResult {
    fn failed(name: &str, iteration: usize, error: String) -> Self {
        Self {
            name: name.to_string(),
            iteration,
            error: Some(error),
            action: Duration::ZERO,
            settle: None,
            unsettled: None,
            cpu: Duration::ZERO,
            peak_cores: 0.0,
            idle_cores: 0.0,
            rss_mb: 0.0,
            threads: 0,
            traffic: Traffic::default(),
            watch: WatchCounts::default(),
            sample: None,
        }
    }
}

/// A file renamed from the editor's explorer: the open buffer is closed under
/// its old name and reopened under the new one.
fn rename_open(s: &mut Session, from: &Path, to: &Path) -> Result<()> {
    let was_open = s.editor().is_open(from);
    std::fs::rename(from, to).map_err(|e| e.to_string())?;
    if was_open {
        s.editor().close(from);
        s.editor().open(to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn insert_on_disk(site: &Site, text: &str) -> Result<()> {
    let mut content = std::fs::read_to_string(&site.path).map_err(|e| e.to_string())?;
    if content.as_bytes().get(site.insert_at.wrapping_sub(1)) != Some(&b'{') {
        return Err(format!(
            "{} no longer matches the base revision",
            site.path.display()
        ));
    }
    content.insert_str(site.insert_at, text);
    std::fs::write(&site.path, content).map_err(|e| e.to_string())
}

/// The capabilities VS Code's language client advertises for the features the
/// server implements. The server changes behaviour on several of them, pull
/// diagnostics above all.
fn initialize_params(root_uri: &str) -> Value {
    json!({
        "processId": std::process::id(),
        "clientInfo": { "name": "graphox-lsp-scenario" },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
        "capabilities": {
            "general": { "positionEncodings": ["utf-16"] },
            "window": { "workDoneProgress": true },
            "workspace": {
                "workspaceFolders": true,
                "configuration": true,
                "applyEdit": true,
                "workspaceEdit": { "documentChanges": true },
                "didChangeWatchedFiles": { "dynamicRegistration": true, "relativePatternSupport": true },
                "diagnostics": { "refreshSupport": true },
                "semanticTokens": { "refreshSupport": true }
            },
            "textDocument": {
                "synchronization": { "dynamicRegistration": true, "didSave": true, "willSave": true },
                "diagnostic": { "dynamicRegistration": true, "relatedDocumentSupport": false },
                "publishDiagnostics": { "relatedInformation": true, "versionSupport": false },
                "completion": {
                    "completionItem": { "snippetSupport": true },
                    "contextSupport": true
                },
                "hover": { "contentFormat": ["markdown", "plaintext"] },
                "definition": { "linkSupport": true },
                "references": {},
                "documentHighlight": {},
                "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                "codeAction": {
                    "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "source"] } }
                },
                "foldingRange": {},
                "rename": { "prepareSupport": true },
                "semanticTokens": {
                    "requests": { "full": { "delta": true }, "range": true },
                    "tokenTypes": [
                        "namespace", "type", "class", "enum", "interface", "struct", "typeParameter",
                        "parameter", "variable", "property", "enumMember", "event", "function",
                        "method", "macro", "keyword", "modifier", "comment", "string", "number",
                        "regexp", "operator", "decorator"
                    ],
                    "tokenModifiers": [
                        "declaration", "definition", "readonly", "static", "deprecated",
                        "abstract", "async", "modification", "documentation", "defaultLibrary"
                    ],
                    "formats": ["relative"]
                }
            }
        }
    })
}
