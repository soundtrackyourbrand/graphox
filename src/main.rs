use clap::{Parser, Subcommand};
use graphox_cli::{run_benchmark, run_check, run_codegen};
use graphox_core::Config;
use graphox_lsp::run_lsp;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Language Server (LSP)
    Lsp,
    /// Scan files for deprecation warnings
    Check {
        /// Directory to scan
        #[arg(default_value = ".")]
        path: String,
        /// Show ignored deprecations
        #[arg(short, long)]
        verbose: bool,
        /// Output format (default, github, tsc)
        #[arg(short, long)]
        reporter: Option<String>,
        /// Lowest severity that fails the run (error, warning, info)
        #[arg(long, default_value = "warning")]
        fail_on: String,
    },
    /// Generate TypeScript types for operations and fragments
    Codegen {
        /// Directory to scan
        #[arg(default_value = ".")]
        path: String,
        /// Watch for changes and re-run codegen
        #[arg(short, long)]
        watch: bool,
        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,
        /// Remove all created codegen files
        #[arg(long)]
        clean: bool,
    },
    /// Benchmark codegen performance
    Benchmark {
        /// Directory to scan
        #[arg(default_value = ".")]
        path: String,
        /// Show detailed fragment discovery information
        #[arg(short, long)]
        verbose: bool,
        /// Write a kill-safe scan trace while benchmarking
        #[arg(long)]
        instrument_scan: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Best-effort, throttled cleanup of the on-disk schema cache so it can't grow
    // without bound (runs on a background thread; no-op if pruned recently).
    //
    // Not for `codegen --clean`, which removes the whole directory a moment
    // later: the two would be deleting each other's entries, and on Windows the
    // prune thread's half-deleted files are what makes the removal fail.
    if !matches!(cli.command, Some(Commands::Codegen { clean: true, .. })) {
        graphox_core::schema_cache::prune_cache_if_due();
    }

    let config = Config::load();

    match cli.command {
        Some(Commands::Lsp) | None => {
            run_lsp(config).await;
        }
        Some(Commands::Check {
            path: _,
            verbose,
            reporter,
            fail_on,
        }) => {
            let reporter: Box<dyn graphox_cli::reporters::Reporter> = match reporter.as_deref() {
                Some("github") => Box::new(graphox_cli::reporters::GitHubReporter),
                Some("tsc") => Box::new(graphox_cli::reporters::TscReporter),
                _ => Box::new(graphox_cli::reporters::DefaultReporter),
            };
            let Some(fail_on) = graphox_core::config::Severity::parse(&fail_on) else {
                eprintln!(
                    "Error: Unknown --fail-on value '{}'. Expected error, warning or info.",
                    fail_on
                );
                graphox_core::utils::flush_stdio();
                std::process::exit(1);
            };
            run_check(config, verbose, reporter, fail_on).await;
        }
        Some(Commands::Codegen {
            path: _,
            watch,
            verbose,
            clean,
        }) => {
            run_codegen(config, watch, verbose, clean).await;
        }
        Some(Commands::Benchmark {
            path: _,
            verbose,
            instrument_scan,
        }) => {
            run_benchmark(config, verbose, instrument_scan).await;
        }
    }
}
