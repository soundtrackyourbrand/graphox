use apollo_compiler::Schema;
use graphql_rust::utils::is_relevant_file;
use graphql_rust::{DocumentLanguage, DocumentState};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

pub async fn run_check(schema_path: &str, scan_path: &str) {
    let schema_text = std::fs::read_to_string(schema_path).expect("Failed to read schema");
    let schema = Schema::parse(&schema_text, schema_path).expect("Failed to parse schema");

    let mut docs = Vec::new();
    println!("Scanning files in {}...", scan_path);

    for entry in WalkBuilder::new(scan_path)
        .add_custom_ignore_filename(".graphqlignore")
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
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
