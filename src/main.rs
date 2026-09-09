use clap::{Parser, Subcommand};
use graphox_cli::{AnalyzeParams, run_analyze, run_benchmark, run_check, run_codegen};
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
    /// Report selections that recur across operations and fragments
    Analyze {
        /// Members a selection must share before it is reported
        #[arg(long, default_value_t = 3)]
        min_fields: usize,
        /// Definitions that must share a selection before it is reported
        #[arg(long, default_value_t = 3)]
        min_uses: usize,
        /// Report only one kind: matches_fragment, extends_fragment, new_fragment
        #[arg(long)]
        kind: Option<String>,
        /// Report only selections on this GraphQL type
        #[arg(long = "type")]
        type_name: Option<String>,
        /// Findings to print per section, or 0 for all. Does not apply to --json
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit findings as JSON
        #[arg(long)]
        json: bool,
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
            // Reporters display paths, so they relativize against the same root
            // the rest of the output does.
            let reporter: Box<dyn graphox_cli::reporters::Reporter> = match reporter.as_deref() {
                Some("github") => {
                    Box::new(graphox_cli::reporters::GitHubReporter::new(config.clone()))
                }
                Some("tsc") => Box::new(graphox_cli::reporters::TscReporter::new(config.clone())),
                _ => Box::new(graphox_cli::reporters::DefaultReporter::new(config.clone())),
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
        Some(Commands::Analyze {
            min_fields,
            min_uses,
            kind,
            type_name,
            limit,
            json,
        }) => {
            let kind = match kind
                .as_deref()
                .map(graphox_cli::commands::analyze::Kind::parse)
            {
                Some(None) => {
                    eprintln!(
                        "Error: Unknown --kind value. Expected matches_fragment, extends_fragment or new_fragment."
                    );
                    graphox_core::utils::flush_stdio();
                    std::process::exit(1);
                }
                parsed => parsed.flatten(),
            };
            run_analyze(
                config,
                AnalyzeParams {
                    min_fields,
                    min_uses,
                    kind,
                    type_name,
                    limit,
                    json,
                },
            )
            .await;
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
