use apollo_compiler::Schema;
use fnv::FnvHashMap as HashMap;
use graphql_rust::utils::{get_project_files, is_relevant_file};
use graphql_rust::{Config, DocumentLanguage, DocumentState};
use std::path::PathBuf;
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

pub async fn run_check(config: Config, verbose: bool) {
    let mut success = true;
    let cfg = config.clone();
    for project in cfg.projects {
        let abs_includes: Vec<String> = project
            .include
            .patterns()
            .iter()
            .map(|p| cfg.base_dir.join(p).to_string_lossy().to_string())
            .collect();
        let abs_excludes: Vec<String> = project
            .exclude
            .as_ref()
            .map(|e| e.patterns())
            .unwrap_or_default()
            .iter()
            .map(|p| cfg.base_dir.join(p).to_string_lossy().to_string())
            .collect();

        println!("Checking project: {}", project.include.as_key());
        if !execute_project_check(
            &cfg.base_dir,
            &project.schema,
            &abs_includes,
            &abs_excludes,
            &config,
            verbose,
        )
        .await
        {
            success = false;
        }
    }

    if !success {
        std::process::exit(1);
    }
}

async fn execute_project_check(
    base_dir: &std::path::Path,
    source: &graphql_rust::config::SchemaSource,
    include_patterns: &[String],
    exclude_patterns: &[String],
    config: &Config,
    verbose: bool,
) -> bool {
    let mut combined_text = String::new();
    for file in source.files() {
        match std::fs::read_to_string(base_dir.join(file)) {
            Ok(t) => {
                combined_text.push_str(&t);
                combined_text.push('\n');
            }
            Err(e) => {
                eprintln!("Failed to read schema {}: {}", source.as_key(), e);
                return false;
            }
        }
    }
    let schema = match Schema::parse(&combined_text, source.as_key()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse schema {}: {}", source.as_key(), e);
            return false;
        }
    };

    let mut docs = Vec::new();
    let paths = get_project_files(include_patterns, exclude_patterns);

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

    let mut fragments_per_package: HashMap<Option<PathBuf>, Vec<graphql_rust::features::completion::FragmentCompletionInfo>> = HashMap::default();
    let mut all_public_fragments: Vec<graphql_rust::features::completion::FragmentCompletionInfo> = Vec::new();
    let mut used_fragments = fnv::FnvHashSet::default();
    let package_roots = dashmap::DashMap::with_hasher(ahash::RandomState::default());

    for (_, doc) in &docs {
        package_roots.insert(doc.uri.clone(), doc.package_root.clone());
        for spread in &doc.fragment_spreads {
            used_fragments.insert(spread.clone());
        }
        for frag in doc.fragments() {
            let info = graphql_rust::features::completion::FragmentCompletionInfo {
                name: frag.name.clone(),
                type_condition: frag.type_condition.clone(),
                description: frag.description.clone(),
                import_path: None,
                is_public: frag.is_public,
                uri: doc.uri.clone(),
                package_root: doc.package_root.clone(),
                used_variables: frag.used_variables.clone(),
                used_fragments: frag.used_fragments.clone(),
            };
            if frag.is_public {
                all_public_fragments.push(info.clone());
            }
            fragments_per_package
                .entry(doc.package_root.clone())
                .or_default()
                .push(info);
        }
    }

    let mut found_any = false;
    for (path, doc) in &docs {
        let mut package_fragments = fragments_per_package
            .get(&doc.package_root)
            .cloned()
            .unwrap_or_default();

        for pub_frag in &all_public_fragments {
            if !package_fragments.iter().any(|f| f.name == pub_frag.name && f.uri == pub_frag.uri) {
                package_fragments.push(pub_frag.clone());
            }
        }

        let diagnostics = doc.get_semantic_diagnostics(
            &schema,
            &package_fragments,
            Some(&used_fragments),
            Some(config),
            verbose,
            true,
        );
        if !diagnostics.is_empty() {
            let mut file_header_printed = false;
            let display_path = if let Some(root) = &doc.package_root {
                path.strip_prefix(root).unwrap_or(path)
            } else {
                path
            };

            for d in diagnostics {
                let is_issue = matches!(
                    d.severity,
                    Some(DiagnosticSeverity::ERROR) | Some(DiagnosticSeverity::WARNING)
                );

                if is_issue || verbose {
                    if is_issue {
                        found_any = true;
                    }

                    if !file_header_printed {
                        println!("\nFile: {}", display_path.display());
                        file_header_printed = true;
                    }

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
    }

    if !found_any {
        if verbose {
            println!("\nScan complete.");
        } else {
            println!("No issues found.");
        }
        true
    } else {
        false
    }
}
