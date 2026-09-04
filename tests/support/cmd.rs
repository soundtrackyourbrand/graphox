//! Helpers for tests that drive the `graphox` binary as a subprocess.

use std::path::{Path, PathBuf};
use std::process::Output;

/// Assert that a `graphox` invocation succeeded, and report enough to diagnose it
/// when it did not.
///
/// `assert!(output.status.success())` reduces a failure to the word "false". On a
/// CI runner that is the entire report, which is not enough to separate a real
/// regression from a environment that got in the way — so every one of these
/// carries the exit status and both streams instead.
#[track_caller]
pub fn assert_command_succeeded(output: &Output, what: &str, dir: &Path) {
    if output.status.success() {
        return;
    }

    panic!(
        "`graphox {what}` failed in {dir}\n  status: {status}\n  stdout:\n{stdout}\n  stderr:\n{stderr}",
        what = what,
        dir = dir.display(),
        status = output.status,
        stdout = block(&String::from_utf8_lossy(&output.stdout)),
        stderr = block(&String::from_utf8_lossy(&output.stderr)),
    );
}

fn block(s: &str) -> String {
    if s.trim().is_empty() {
        return "    <empty>".to_string();
    }
    s.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A scratch directory for one test run, named so that no other run can be using
/// it.
///
/// A fixed name under the system temp dir is shared with every other run on the
/// machine, and CI runners are reused. Removing the leftovers first only works
/// while the removal does — on Windows a file still held open makes
/// `remove_dir_all` fail, and the test then builds its fixture on top of whatever
/// survived. The process id and a timestamp make that collision impossible
/// instead of unlikely.
///
/// Failures to prepare the directory panic here rather than being discarded, so a
/// test that cannot get a clean fixture says so at the point it happened.
pub fn fresh_dir(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{name}_{}_{stamp}", std::process::id()));

    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .unwrap_or_else(|e| panic!("could not clear {}: {e}", dir.display()));
    }
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", dir.display()));

    dir
}
