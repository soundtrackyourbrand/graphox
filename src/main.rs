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
    },
    /// Generate TypeScript types for operations and fragments
    Codegen {
        /// Directory to scan
        #[arg(default_value = ".")]
        path: String,
        /// Output directory (default: next to input files)
        #[arg(short, long)]
        output: Option<String>,
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
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = Config::load();

    match cli.command {
        Some(Commands::Lsp) | None => {
            run_lsp(config).await;
        }
        Some(Commands::Check {
            path: _,
            verbose,
            reporter,
        }) => {
            let reporter: Box<dyn graphox_cli::reporters::Reporter> = match reporter.as_deref() {
                Some("github") => Box::new(graphox_cli::reporters::GitHubReporter),
                Some("tsc") => Box::new(graphox_cli::reporters::TscReporter),
                _ => Box::new(graphox_cli::reporters::DefaultReporter),
            };
            run_check(config, verbose, reporter).await;
        }
        Some(Commands::Codegen {
            path: _,
            output,
            watch,
            verbose,
            clean,
        }) => {
            run_codegen(config, output.as_deref(), watch, verbose, clean).await;
        }
        Some(Commands::Benchmark { path: _, verbose }) => {
            run_benchmark(config, verbose).await;
        }
    }
}
