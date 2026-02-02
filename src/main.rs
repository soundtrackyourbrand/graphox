mod commands;

use clap::{Parser, Subcommand};
use commands::check::run_check;
use commands::codegen::run_codegen;
use commands::lsp::run_lsp;

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
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Lsp) | None => {
            run_lsp(&cli.schema).await;
        }
        Some(Commands::Check { path }) => {
            run_check(&cli.schema, &path).await;
        }
        Some(Commands::Codegen { path, output, watch }) => {
            run_codegen(&cli.schema, &path, output.as_deref(), watch).await;
        }
    }
}
