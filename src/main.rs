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

    // Group fragments by package root, but also collect all public fragments
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

        // Add public fragments from other packages
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

fn is_relevant_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext {
        "graphql" | "gql" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => true,
        _ => false,
    }
}
