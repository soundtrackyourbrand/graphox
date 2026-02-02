use apollo_compiler::Schema;
use clap::{Parser, Subcommand};
use graphql_rust::{Backend, DocumentLanguage, DocumentState};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};
use tower_lsp::{LspService, Server};
use walkdir::WalkDir;

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
            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();
            let (service, socket) = LspService::new(|client| Backend::new(client, &cli.schema));
            Server::new(stdin, stdout, socket).serve(service).await;
        }
        Some(Commands::Check { path }) => {
            run_check(&cli.schema, &path).await;
        }
        Some(Commands::Codegen { path, output, watch }) => {
            run_codegen(&cli.schema, &path, output.as_deref(), watch).await;
        }
    }
}

async fn run_check(schema_path: &str, scan_path: &str) {
    let schema_text = std::fs::read_to_string(schema_path).expect("Failed to read schema");
    let schema = Schema::parse(&schema_text, schema_path).expect("Failed to parse schema");

    let mut docs = Vec::new();
    println!("Scanning files in {}...", scan_path);

    for entry in WalkDir::new(scan_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path().to_owned();
        if is_relevant_file(&path) {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let uri = Url::from_file_path(std::fs::canonicalize(&path).unwrap()).unwrap();

            let language = DocumentLanguage::from_uri(&uri);
            let mut ts_parser = tree_sitter::Parser::new();
            ts_parser
                .set_language(&language.get_parser_language())
                .unwrap();

            let doc = DocumentState::new(uri, &content, ts_parser);
            docs.push((path, doc));
        }
    }

    let mut fragments_per_package: HashMap<Option<PathBuf>, Vec<String>> = HashMap::new();
    let mut all_public_fragments: Vec<String> = Vec::new();

    for (_, doc) in &docs {
        for frag in doc.fragments() {
            if frag.is_public {
                all_public_fragments.push(frag.name.clone());
            }
            fragments_per_package
                .entry(doc.package_root.clone())
                .or_default()
                .push(frag.name.clone());
        }
    }

    let mut found_any = false;
    for (path, doc) in &docs {
        let mut package_fragments = fragments_per_package
            .get(&doc.package_root)
            .cloned()
            .unwrap_or_default();

        for pub_frag in &all_public_fragments {
            if !package_fragments.contains(pub_frag) {
                package_fragments.push(pub_frag.clone());
            }
        }

        let diagnostics = doc.get_semantic_diagnostics(&schema, &package_fragments);
        if !diagnostics.is_empty() {
            found_any = true;
            let display_path = path.strip_prefix(scan_path).unwrap_or(&path);
            println!("\nFile: {}", display_path.display());
            for d in diagnostics {
                let severity = match d.severity {
                    Some(DiagnosticSeverity::ERROR) => "Error",
                    Some(DiagnosticSeverity::WARNING) => "Warning",
                    Some(DiagnosticSeverity::INFORMATION) => "Info",
                    Some(DiagnosticSeverity::HINT) => "Hint",
                    _ => "Diagnostic",
                };
                println!(
                    "  [{}:{}] {}: {}",
                    d.range.start.line + 1,
                    d.range.start.character + 1,
                    severity,
                    d.message
                );
            }
        }
    }

    if !found_any {
        println!("No issues found.");
    } else {
        std::process::exit(1);
    }
}

async fn run_codegen(schema_path: &str, scan_path: &str, output_dir: Option<&str>, watch: bool) {
    if !watch {
        execute_codegen(schema_path, scan_path, output_dir).await;
        return;
    }

    println!("Watching for changes in {}...", scan_path);
    execute_codegen(schema_path, scan_path, output_dir).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    let mut debouncer = notify_debouncer_mini::new_debouncer(
        std::time::Duration::from_millis(200),
        move |res: notify_debouncer_mini::DebounceEventResult| match res {
            Ok(_) => {
                let _ = tx.blocking_send(());
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        },
    )
    .expect("Failed to create debouncer");

    debouncer
        .watcher()
        .watch(Path::new(scan_path), notify::RecursiveMode::Recursive)
        .expect("Failed to watch directory");
    
    // Also watch schema
    debouncer
        .watcher()
        .watch(Path::new(schema_path), notify::RecursiveMode::NonRecursive)
        .expect("Failed to watch schema");

    while let Some(_) = rx.recv().await {
        println!("\nChange detected, re-running codegen...");
        execute_codegen(schema_path, scan_path, output_dir).await;
    }
}

async fn execute_codegen(schema_path: &str, scan_path: &str, output_dir: Option<&str>) {
    let schema_text = std::fs::read_to_string(schema_path).expect("Failed to read schema");
    let schema = Schema::parse(&schema_text, schema_path).expect("Failed to parse schema");

    let mut docs = Vec::new();

    for entry in WalkDir::new(scan_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path().to_owned();
        if is_relevant_file(&path) {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let uri = Url::from_file_path(std::fs::canonicalize(&path).unwrap()).unwrap();
            let language = DocumentLanguage::from_uri(&uri);
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&language.get_parser_language())
                .unwrap();
            let doc = DocumentState::new(uri, &content, parser);
            if !doc.get_graphql_trees().is_empty() {
                docs.push((path, doc));
            }
        }
    }

    let mut fragment_to_path = HashMap::new();
    for (path, doc) in &docs {
        for frag in doc.fragments() {
            fragment_to_path.insert(frag.name.clone(), path.to_string_lossy().to_string());
        }
    }

    for (path, doc) in &docs {
        let ctx = graphql_rust::features::codegen::CodegenContext {
            schema: &schema,
            fragment_to_path: &fragment_to_path,
            current_file_path: &path,
        };

        match graphql_rust::features::codegen::generate_typescript(doc, &ctx) {
            Ok(ts_code) => {
                let out_path = if let Some(dir) = output_dir {
                    let rel = path.strip_prefix(scan_path).unwrap_or(&path);
                    let mut p = PathBuf::from(dir);
                    p.push(rel);
                    p.set_extension("graphql.codegen.ts");
                    p
                } else {
                    let mut p = path.clone();
                    let mut filename = p.file_name().unwrap().to_os_string();
                    filename.push(".codegen.ts");
                    p.set_file_name(filename);
                    p
                };

                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&out_path, ts_code).expect("Failed to write codegen file");
                println!("Generated: {}", out_path.display());
            }
            Err(e) => {
                eprintln!("Error generating types for {}: {}", path.display(), e);
            }
        }
    }
}

fn is_relevant_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext {
        "graphql" | "gql" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => true,
        _ => false,
    }
}
