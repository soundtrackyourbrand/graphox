mod commands;

use clap::{Parser, Subcommand};
use commands::check::run_check;
use commands::codegen::run_codegen;
use commands::lsp::run_lsp;
use commands::benchmark::run_benchmark;
use graphql_rust::Config;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the GraphQL schema file
    #[arg(short, long, default_value = "schema.graphql")]
    schema: String,
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
            run_lsp(config, &cli.schema).await;
        }
        Some(Commands::Check { path, verbose }) => {
            run_check(config, &cli.schema, &path, verbose).await;
        }
        Some(Commands::Codegen {
            path,
            output,
            watch,
            verbose,
            clean,
        }) => {
            run_codegen(config, &cli.schema, &path, output.as_deref(), watch, verbose, clean).await;
        }
        Some(Commands::Benchmark { path, verbose }) => {
            run_benchmark(config, &cli.schema, &path, verbose).await;
        }
    }
}
