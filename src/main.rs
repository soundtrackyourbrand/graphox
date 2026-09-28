use clap::{Parser, Subcommand};
use graphox_cli::{
    AnalyzeParams, CodegenWeightParams, ExpandParams, OperationsParams, UsageParams, run_analyze,
    run_benchmark, run_check, run_codegen, run_codegen_weight, run_expand, run_operations,
    run_usage,
};
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
        /// Report only files under this directory. The whole workspace is
        /// still validated, so fragments from elsewhere resolve. Without it,
        /// every file is reported
        path: Option<String>,
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
        /// Generate only the projects and schema_types under this directory.
        /// Generating only schema_types skips the workspace scan. Without it,
        /// everything is generated
        path: Option<String>,
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
    /// Inspect how the workspace uses GraphQL
    Analyze {
        #[command(subcommand)]
        tool: AnalyzeTool,
    },
    /// Benchmark codegen performance
    Benchmark {
        /// Show detailed fragment discovery information
        #[arg(short, long)]
        verbose: bool,
        /// Write a kill-safe scan trace while benchmarking
        #[arg(long)]
        instrument_scan: bool,
    },
}

#[derive(Subcommand)]
enum AnalyzeTool {
    /// Report selections that recur across operations and fragments
    Selections {
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
    /// Report which types and fields the workspace actually selects
    Usage {
        /// Restrict to projects whose include path contains this
        #[arg(long, visible_alias = "project")]
        app: Option<String>,
        /// Report only this GraphQL type, listing its fields by consumer count
        #[arg(long = "type")]
        type_name: Option<String>,
        /// Report only this field, listing what selects it
        #[arg(long)]
        field: Option<String>,
        /// Report only declared fields that nothing selects
        #[arg(long)]
        unused: bool,
        /// Rows to print, or 0 for all. Does not apply to --json
        #[arg(long, default_value_t = 30)]
        limit: usize,
        /// Emit results as JSON
        #[arg(long)]
        json: bool,
    },
    /// Report what each operation asks the server for
    Operations {
        /// Restrict to projects whose include path contains this
        #[arg(long, visible_alias = "project")]
        app: Option<String>,
        /// Report only one kind: query, mutation, subscription
        #[arg(long)]
        kind: Option<String>,
        /// Explain this operation instead of ranking all of them
        #[arg(long)]
        name: Option<String>,
        /// Order by: depth, fields, lists
        #[arg(long, default_value = "depth")]
        sort: String,
        /// Rows to print, or 0 for all. Does not apply to --json
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit results as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print an operation as the server receives it, with every fragment it spreads
    Expand {
        /// Name of the operation to expand
        operation: String,
        /// A project directory, or a file in the project to resolve it in.
        /// Every project is searched without one
        path: Option<String>,
        /// Print the request body a client posts: operationName and query
        #[arg(long)]
        json: bool,
    },
    /// Report what the generated TypeScript weighs, per definition
    Codegen {
        /// Restrict to projects whose include path contains this
        #[arg(long, visible_alias = "project")]
        app: Option<String>,
        /// Report only one kind: operation, fragment
        #[arg(long)]
        kind: Option<String>,
        /// Order by: bytes, ast
        #[arg(long, default_value = "bytes")]
        sort: String,
        /// Rows to print, or 0 for all. Does not apply to --json
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit results as JSON
        #[arg(long)]
        json: bool,
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
            path,
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
            let scope = scope_from(path.as_deref(), &config);
            run_check(config, verbose, reporter, fail_on, scope).await;
        }
        Some(Commands::Codegen {
            path,
            watch,
            verbose,
            clean,
        }) => {
            let scope = scope_from(path.as_deref(), &config);
            run_codegen(config, watch, verbose, clean, scope).await;
        }
        Some(Commands::Analyze { tool }) => match tool {
            AnalyzeTool::Selections {
                min_fields,
                min_uses,
                kind,
                type_name,
                limit,
                json,
            } => {
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
            AnalyzeTool::Usage {
                app,
                type_name,
                field,
                unused,
                limit,
                json,
            } => {
                run_usage(
                    config,
                    UsageParams {
                        app,
                        type_name,
                        field,
                        unused,
                        limit,
                        json,
                    },
                )
                .await;
            }
            AnalyzeTool::Operations {
                app,
                kind,
                name,
                sort,
                limit,
                json,
            } => {
                let kind = match kind
                    .as_deref()
                    .map(graphox_features::analysis::operations::OperationKind::parse)
                {
                    Some(None) => {
                        eprintln!(
                            "Error: Unknown --kind value. Expected query, mutation or subscription."
                        );
                        graphox_core::utils::flush_stdio();
                        std::process::exit(1);
                    }
                    parsed => parsed.flatten(),
                };
                let Some(sort) = graphox_cli::commands::operations::Sort::parse(&sort) else {
                    eprintln!(
                        "Error: Unknown --sort value '{sort}'. Expected depth, fields or lists."
                    );
                    graphox_core::utils::flush_stdio();
                    std::process::exit(1);
                };
                run_operations(
                    config,
                    OperationsParams {
                        app,
                        kind,
                        name,
                        sort,
                        limit,
                        json,
                    },
                )
                .await;
            }
            AnalyzeTool::Expand {
                operation,
                path,
                json,
            } => {
                let scope = scope_from(path.as_deref(), &config);
                run_expand(
                    config,
                    ExpandParams {
                        operation,
                        scope,
                        json,
                    },
                )
                .await;
            }
            AnalyzeTool::Codegen {
                app,
                kind,
                sort,
                limit,
                json,
            } => {
                let kind = match kind
                    .as_deref()
                    .map(graphox_cli::commands::codegen_weight::Kind::parse)
                {
                    Some(None) => {
                        eprintln!("Error: Unknown --kind value. Expected operation or fragment.");
                        graphox_core::utils::flush_stdio();
                        std::process::exit(1);
                    }
                    parsed => parsed.flatten(),
                };
                let Some(sort) = graphox_cli::commands::codegen_weight::Sort::parse(&sort) else {
                    eprintln!("Error: Unknown --sort value '{sort}'. Expected bytes or ast.");
                    graphox_core::utils::flush_stdio();
                    std::process::exit(1);
                };
                run_codegen_weight(
                    config,
                    CodegenWeightParams {
                        app,
                        kind,
                        sort,
                        limit,
                        json,
                    },
                )
                .await;
            }
        },
        Some(Commands::Benchmark {
            verbose,
            instrument_scan,
        }) => {
            run_benchmark(config, verbose, instrument_scan).await;
        }
    }
}

/// The `[PATH]` argument as a scope, or the whole workspace without one.
fn scope_from(path: Option<&str>, config: &Config) -> Option<graphox_cli::PathScope> {
    let path = path?;
    match graphox_cli::PathScope::new(path, config) {
        Ok(scope) => Some(scope),
        Err(message) => {
            eprintln!("Error: {message}");
            graphox_core::utils::flush_stdio();
            std::process::exit(1);
        }
    }
}
