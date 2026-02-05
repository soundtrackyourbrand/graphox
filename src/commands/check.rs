use apollo_compiler::Schema;
use colored::*;
use fnv::FnvHashMap as HashMap;
use graphql_rust::engine::Engine;
use graphql_rust::{Config, DocumentState};
use std::path::PathBuf;
use tower_lsp::lsp_types::DiagnosticSeverity;

pub async fn run_check(config: Config, verbose: bool) {
    let mut success = true;
    let cfg = config.clone();

    println!("{}", "Scanning workspace...".bright_black());
    let workspace_metadata = Engine::scan_workspace(&cfg, |_, _| {});

    let mut global_used_fragments = fnv::FnvHashSet::default();
    for doc in workspace_metadata.documents.values() {
        for spread in &doc.fragment_spreads {
            global_used_fragments.insert(spread.clone());
        }
    }

    let mut global_public_fragments: Vec<
        graphql_rust::features::completion::FragmentCompletionInfo,
    > = Vec::new();

    for doc in workspace_metadata.documents.values() {
        let package_root = doc.package_root.clone();
        let project_import = cfg
            .get_project_for_path(&doc.uri.to_file_path().unwrap_or_default())
            .and_then(|p| p.import.clone());

        for frag in doc.fragments() {
            if frag.is_public {
                global_public_fragments.push(
                    graphql_rust::features::completion::FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: frag.description.clone(),
                        import_path: project_import.clone(),
                        is_public: frag.is_public,
                        is_type_only: frag.is_type_only,
                        uri: doc.uri.clone(),
                        package_root: package_root.clone(),
                        used_variables: frag.used_variables.clone(),
                        used_fragments: frag.used_fragments.clone(),
                        requirements: std::collections::BTreeMap::new(),
                    },
                );
            }
        }
    }

    for (project_config, project_meta) in cfg.projects.iter().zip(&workspace_metadata.projects) {
        println!(
            "Checking project: {}",
            project_config.include.as_key().blue()
        );

        if !execute_project_check(
            &cfg.base_dir,
            &project_config.schema,
            &project_meta.files,
            &workspace_metadata.documents,
            &global_used_fragments,
            &global_public_fragments,
            &config,
            verbose,
        )
        .await
        {
            success = false;
        }
    }

    if !success {
        println!("{}", "\nCheck failed.".red());
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_project_check(
    base_dir: &std::path::Path,
    source: &graphql_rust::config::SchemaSource,
    project_files: &[PathBuf],
    all_documents: &HashMap<PathBuf, DocumentState>,
    global_used_fragments: &fnv::FnvHashSet<String>,
    global_public_fragments: &[graphql_rust::features::completion::FragmentCompletionInfo],
    config: &Config,
    verbose: bool,
) -> bool {
    let mut texts = Vec::new();
    for file in source.files() {
        match std::fs::read_to_string(base_dir.join(file)) {
            Ok(t) => {
                texts.push(t);
            }
            Err(e) => {
                eprintln!(
                    "{} {}: {}",
                    "Failed to read schema".red(),
                    source.as_key().blue(),
                    e.to_string().red()
                );
                return false;
            }
        }
    }
    let combined_text = graphql_rust::utils::merge_schema_texts(&texts);
    let schema = match Schema::parse(&combined_text, source.as_key()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "{} {}: {}",
                "Failed to parse schema".red(),
                source.as_key().blue(),
                e.to_string().red()
            );
            return false;
        }
    };
    let valid_schema = match schema.validate() {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "{} {}: {}",
                "Schema validation failed".red(),
                source.as_key().blue(),
                e.to_string().red()
            );
            return false;
        }
    };

    let mut found_any = false;

    let mut project_fragments = Vec::new();
    for path in project_files {
        if let Some(doc) = all_documents.get(path) {
            for frag in doc.fragments() {
                project_fragments.push(
                    graphql_rust::features::completion::FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: frag.description.clone(),
                        import_path: None,
                        is_public: frag.is_public,
                        is_type_only: frag.is_type_only,
                        uri: doc.uri.clone(),
                        package_root: doc.package_root.clone(),
                        used_variables: frag.used_variables.clone(),
                        used_fragments: frag.used_fragments.clone(),
                        requirements: std::collections::BTreeMap::new(),
                    },
                );
            }
        }
    }

    for (path, doc) in project_files
        .iter()
        .zip(project_files.iter().filter_map(|p| all_documents.get(p)))
    {
        let mut available_fragments = project_fragments.clone();

        for pub_frag in global_public_fragments {
            if !available_fragments
                .iter()
                .any(|f| f.name == pub_frag.name && f.uri == pub_frag.uri)
            {
                available_fragments.push(pub_frag.clone());
            }
        }

        // Filter fragments for this doc (same project or public)
        // Note: we already have all project fragments in available_fragments.
        // We still might want to prioritize the ones in the same package if there are name collisions,
        // but for now let's just make them all available.

        // If there are duplicate fragment names in the project, we should probably
        // prefer the one in the same package.
        available_fragments.sort_by(|a, b| {
            let a_same_pkg = a.package_root == doc.package_root;
            let b_same_pkg = b.package_root == doc.package_root;
            b_same_pkg.cmp(&a_same_pkg)
        });

        let diagnostics = doc.get_semantic_diagnostics(
            &valid_schema,
            &available_fragments,
            Some(global_used_fragments),
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
                        println!("\nFile: {}", display_path.display().to_string().blue());
                        file_header_printed = true;
                    }

                    let (severity_label, colored_msg) = match d.severity {
                        Some(DiagnosticSeverity::ERROR) => ("Error".red(), d.message.red()),
                        Some(DiagnosticSeverity::WARNING) => {
                            ("Warning".yellow(), d.message.yellow())
                        }
                        Some(DiagnosticSeverity::INFORMATION) => {
                            ("Info".bright_black(), d.message.bright_black())
                        }
                        Some(DiagnosticSeverity::HINT) => {
                            ("Hint".bright_black(), d.message.bright_black())
                        }
                        _ => ("Diagnostic".normal(), d.message.normal()),
                    };
                    println!(
                        "  [{}:{}] {}: {}",
                        (d.range.start.line + 1).to_string().bright_black(),
                        (d.range.start.character + 1).to_string().bright_black(),
                        severity_label,
                        colored_msg
                    );
                }
            }
        }
    }

    if !found_any {
        if verbose {
            println!("\n{}", "Scan complete.".bright_black());
        } else {
            println!("{}", "No issues found.".green());
        }
        true
    } else {
        false
    }
}
