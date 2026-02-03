use apollo_compiler::Schema;
use graphql_rust::utils::{get_project_files, is_relevant_file};
use graphql_rust::{Config, DocumentLanguage, DocumentState};
use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

pub async fn run_check(config: Option<Config>, schema_path: &str, scan_path: &str) {
    let mut success = true;
    if let Some(cfg) = config {
        for project in cfg.projects {
            let abs_schema = cfg.base_dir.join(&project.schema);
            let abs_include = cfg.base_dir.join(&project.include);
            println!("Checking project with schema: {}", project.schema);
            if !execute_project_check(&abs_schema.to_string_lossy(), &abs_include.to_string_lossy()).await {
                success = false;
            }
        }
    } else {
        let include = if std::path::Path::new(scan_path).is_file() {
            scan_path.to_string()
        } else {
            format!("{}/**/*", scan_path)
        };
        if !execute_project_check(schema_path, &include).await {
            success = false;
        }
    }

    if !success {
        std::process::exit(1);
    }
}

async fn execute_project_check(schema_path: &str, include_glob: &str) -> bool {
    let schema_text = match std::fs::read_to_string(schema_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to read schema {}: {}", schema_path, e);
            return false;
        }
    };
    let schema = match Schema::parse(&schema_text, schema_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse schema {}: {}", schema_path, e);
            return false;
        }
    };

    let mut docs = Vec::new();
    let paths = get_project_files(include_glob);

    for path in paths {
        if is_relevant_file(&path) {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let uri = match std::fs::canonicalize(&path) {
                Ok(p) => Url::from_file_path(p).unwrap(),
                Err(_) => continue,
            };

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
            let display_path = if let Some(root) = &doc.package_root {
                path.strip_prefix(root).unwrap_or(path)
            } else {
                path
            };
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
        true
    } else {
        false
    }
}
