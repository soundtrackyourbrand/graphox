//! Drives a real `graphox lsp` process through an editing session on a real
//! repository — typing, renames, file moves, stashes, branch switches, pulls,
//! rebases, schema edits — and reports what each step costs the server and
//! whether it goes quiet afterwards.
//!
//! The session runs in a disposable clone, never in the repository given.

mod editor;
mod lsp;
mod process_stats;
mod pull_diagnostics;
mod report;
mod scenario;
mod watcher;
mod workspace;

use clap::Parser;
use scenario::{Options, Session, Step};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use watcher::GlobBase;

const DEFAULT_STEPS: &[Step] = &[
    Step::OpenFiles,
    Step::TypeOperation,
    Step::TypeFragment,
    Step::RenameFragment,
    Step::MoveFile,
    Step::ExternalEdits,
    Step::BranchSwitch,
    Step::Pull,
    Step::Rebase,
    Step::SchemaChange,
    Step::Idle,
    Step::Restart,
];

#[derive(Parser)]
#[command(about = "Replay an editing session against the graphox language server")]
struct Args {
    /// Repository to run against. It is cloned; the original is never touched
    #[arg(long)]
    repo: PathBuf,
    /// Revision of the repository to start from
    #[arg(long, default_value = "HEAD")]
    rev: String,
    /// Server binary. Defaults to the `graphox` next to this binary
    #[arg(long)]
    server: Option<PathBuf>,
    /// Where the clone and run reports are kept
    #[arg(long, default_value = "target/lsp-scenario")]
    work_dir: PathBuf,
    /// Steps to run after startup, in order
    #[arg(long, value_enum, value_delimiter = ',')]
    steps: Option<Vec<Step>>,
    /// Run the steps this many times over, to expose work that grows
    #[arg(long, default_value_t = 1)]
    repeat: usize,
    /// Branch for the branch-switch step. Defaults to the recent branch whose
    /// checkout rewrites the most GraphQL-bearing files
    #[arg(long)]
    branch: Option<String>,
    /// How far back the pull and rebase steps start
    #[arg(long, default_value_t = 100)]
    pull_commits: usize,
    /// Files edited outside the editor in the external-edits step
    #[arg(long, default_value_t = 20)]
    bulk_files: usize,
    /// Cores under which the server counts as quiet
    #[arg(long, default_value_t = 0.05)]
    threshold: f64,
    /// Seconds the server must stay quiet for a step to have settled
    #[arg(long, default_value_t = 3)]
    quiet_secs: u64,
    /// Seconds to wait for a step to settle before sampling its stacks
    #[arg(long, default_value_t = 120)]
    timeout_secs: u64,
    /// Seconds measured after each step settles
    #[arg(long, default_value_t = 5)]
    idle_secs: u64,
    /// Length of the idle step
    #[arg(long, default_value_t = 30)]
    idle_step_secs: u64,
    /// Delay before re-pulling workspace diagnostics after a response
    #[arg(long, default_value_t = 2000)]
    poll_interval_ms: u64,
    /// Window over which file system events are batched into one notification
    #[arg(long, default_value_t = 250)]
    watch_batch_ms: u64,
    /// How string watcher globs are matched
    #[arg(long, value_enum, default_value = "absolute")]
    glob_base: GlobBase,
    /// Delay between typed characters
    #[arg(long, default_value_t = 80)]
    keystroke_ms: u64,
    /// Sample stacks during every step, not only those that do not settle (macOS)
    #[arg(long)]
    sample_all: bool,
    /// Seconds of stack sampling for a step that does not settle (macOS)
    #[arg(long, default_value_t = 5)]
    sample_secs: u64,
}

fn main() {
    let args = Args::parse();
    if process_stats::snapshot(std::process::id()).is_none() {
        eprintln!("reading process CPU time is not supported on this platform");
        std::process::exit(2);
    }

    let server = args.server.clone().unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| Some(p.parent()?.join("graphox")))
            .unwrap_or_else(|| PathBuf::from("graphox"))
    });
    if !server.is_file() {
        eprintln!(
            "server binary {} not found; build it or pass --server",
            server.display()
        );
        std::process::exit(2);
    }

    let source = args
        .repo
        .canonicalize()
        .unwrap_or_else(|e| fail(format!("{}: {e}", args.repo.display())));
    eprintln!("preparing a clone of {} ...", source.display());
    let (repo, base) =
        workspace::prepare(&source, &args.work_dir, &args.rev).unwrap_or_else(|e| fail(e));

    let config = match graphox_core::Config::load_from_dir(&repo) {
        Ok(Some(c)) => c,
        Ok(None) => fail(format!(
            "no graphox.yaml at the root of {}",
            source.display()
        )),
        Err((path, e)) => fail(format!("{}: {e}", path.display())),
    };
    let targets = workspace::discover(&repo, &config, args.bulk_files).unwrap_or_else(|e| fail(e));
    eprintln!(
        "{} project files, {} with GraphQL; editing operation {} and fragment {} ({} consumers)",
        targets.project_documents,
        targets.graphql_documents,
        targets.operation.name,
        targets.fragment.name,
        targets.fragment_consumers
    );

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let out_dir = args.work_dir.join("runs").join(stamp.to_string());
    std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| fail(e.to_string()));
    let log =
        lsp::Log::create(&out_dir.join("session.log")).unwrap_or_else(|e| fail(e.to_string()));

    let opts = Options {
        server,
        repo,
        base,
        out_dir: out_dir.clone(),
        quiet: Duration::from_secs(args.quiet_secs),
        threshold: args.threshold,
        timeout: Duration::from_secs(args.timeout_secs),
        idle: Duration::from_secs(args.idle_secs),
        idle_long: Duration::from_secs(args.idle_step_secs),
        poll_interval: Duration::from_millis(args.poll_interval_ms),
        watch_batch: Duration::from_millis(args.watch_batch_ms),
        glob_base: args.glob_base,
        keystroke: Duration::from_millis(args.keystroke_ms),
        pull_commits: args.pull_commits,
        branch: args.branch.clone(),
        sample_secs: args.sample_secs,
        sample_all: args.sample_all,
    };

    let steps = args.steps.clone().unwrap_or_else(|| DEFAULT_STEPS.to_vec());
    let mut session = Session::new(opts, config, targets, log);
    eprintln!(
        "running scenario; log in {}",
        out_dir.join("session.log").display()
    );
    session.startup();
    if session.results.last().is_some_and(|r| r.error.is_none()) {
        session.report_dead_watchers();
        for iteration in 0..args.repeat.max(1) {
            session.iteration = iteration;
            for step in &steps {
                session.run(*step);
            }
        }
    }
    session.finish();

    report::print_table(&session.results, session.opts.threshold);
    let json_path = out_dir.join("report.json");
    let _ = std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&report::to_json(&session.results)).unwrap_or_default(),
    );
    println!(
        "\nreport: {}\nlog:    {}",
        json_path.display(),
        out_dir.join("session.log").display()
    );
}

fn fail(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}
