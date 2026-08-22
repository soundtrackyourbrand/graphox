//! End-to-end coverage for `graphox codegen --watch`.
//!
//! The debounced watcher is the one place `notify-debouncer-mini` is used, and
//! its callback runs on notify's own thread, so no unit test reaches the wiring.
//! `classify_watch_events` covers the decision logic; these tests cover the rest
//! of the path — that a real filesystem change reaches that decision, and that
//! codegen actually re-runs.
//!
//! Marked `#[ignore]` because they spawn the binary and wait on real filesystem
//! events, which makes them slower and more timing-dependent than the rest of the
//! suite. CI runs them through the same `--ignored` pass as the other
//! long-running integration tests.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SCHEMA: &str = "type Query { me: User }\ntype User { id: ID! username: String! }\n";
const CONFIG: &str = "projects:\n  - schema: \"schema.graphql\"\n    include: \"**/*.graphql\"\n    output_dir: \"gen\"\n";

fn setup(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).ok();
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("schema.graphql"), SCHEMA).unwrap();
    std::fs::write(dir.join("graphox.yaml"), CONFIG).unwrap();
    std::fs::write(
        dir.join("query.graphql"),
        "query GetMe {\n  me {\n    id\n  }\n}\n",
    )
    .unwrap();
    dir
}

/// A watcher child process that is killed when the guard drops, so a failing
/// assertion cannot leave a `graphox codegen --watch` running.
struct Watcher {
    child: Child,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Watcher {
    fn spawn(dir: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_graphox"))
            .current_dir(dir)
            .args(["codegen", "--watch"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn the watcher");
        Self { child }
    }

    fn stderr(&mut self) -> String {
        let mut buf = String::new();
        if let Some(err) = self.child.stderr.as_mut() {
            err.read_to_string(&mut buf).ok();
        }
        buf
    }
}

/// Poll until `check` passes or the deadline expires. Filesystem events have no
/// completion signal, so waiting on the effect is the only option; polling keeps
/// the common case fast instead of sleeping for the worst case.
fn wait_until(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
#[ignore = "spawns the binary and waits on filesystem events"]
fn watch_regenerates_when_a_document_changes() {
    let dir = setup("graphox_watch_regenerates");
    let generated = dir.join("gen/graphql.ts");
    let mut watcher = Watcher::spawn(&dir);

    assert!(
        wait_until(Duration::from_secs(30), || generated.exists()),
        "initial codegen never produced {}. stderr: {}",
        generated.display(),
        watcher.stderr()
    );
    assert!(
        !read(&generated).contains("WatchProbe"),
        "the probe operation should not exist before it is written"
    );

    std::fs::write(
        dir.join("probe.graphql"),
        "query WatchProbe {\n  me {\n    username\n  }\n}\n",
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(30), || read(&generated)
            .contains("WatchProbe")),
        "adding a document did not reach the generated output. stderr: {}",
        watcher.stderr()
    );
}

/// Asserts an absence, so it would also pass if the watcher were dead. That is
/// covered by `watch_regenerates_when_a_document_changes` in this same file:
/// a dead watcher fails there, loudly, in the same run. No positive control is
/// duplicated here — but the two are a pair, and removing that one leaves this
/// one unable to fail.
#[test]
#[ignore = "spawns the binary and waits on filesystem events"]
fn watch_ignores_its_own_output() {
    let dir = setup("graphox_watch_ignores_output");
    let generated = dir.join("gen/graphql.ts");
    let mut watcher = Watcher::spawn(&dir);

    assert!(
        wait_until(Duration::from_secs(30), || generated.exists()),
        "initial codegen never ran. stderr: {}",
        watcher.stderr()
    );

    // Touching a generated file must not be taken for a source change, or the
    // watcher would regenerate in response to its own writes, forever.
    let before = std::fs::metadata(&generated).unwrap().modified().unwrap();
    let sentinel = dir.join("gen/sentinel.codegen.ts");
    std::fs::write(&sentinel, "// @generated\nexport const x = 1;\n").unwrap();

    std::thread::sleep(Duration::from_secs(3));
    let after = std::fs::metadata(&generated).unwrap().modified().unwrap();
    assert_eq!(
        before, after,
        "writing into the output directory triggered a regeneration"
    );
}

#[test]
#[ignore = "spawns the binary and waits on filesystem events"]
fn watch_reloads_when_the_config_changes() {
    let dir = setup("graphox_watch_reloads_config");
    let generated = dir.join("gen/graphql.ts");
    let relocated = dir.join("out/graphql.ts");
    let mut watcher = Watcher::spawn(&dir);

    assert!(
        wait_until(Duration::from_secs(30), || generated.exists()),
        "initial codegen never ran. stderr: {}",
        watcher.stderr()
    );

    // Point output_dir somewhere else: only a genuine config reload produces
    // files under the new directory.
    std::fs::write(
        dir.join("graphox.yaml"),
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"**/*.graphql\"\n    output_dir: \"out\"\n",
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(30), || relocated.exists()),
        "the config change did not take effect. stderr: {}",
        watcher.stderr()
    );
}
