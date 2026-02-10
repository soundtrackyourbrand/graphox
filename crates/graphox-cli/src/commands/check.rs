use crate::reporters::Reporter;
use ahash::AHashMap as HashMap;
use apollo_compiler::Schema;
use colored::*;
use graphox_core::config::SchemaSource;
use graphox_core::engine::Engine;
use graphox_core::utils;
use graphox_core::{Config, DocumentState};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::diagnostics::DocumentDiagnostics;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::DiagnosticSeverity;

pub async fn run_check(config: Config, verbose: bool, reporter: Box<dyn Reporter>) {
    let mut success = true;
    let cfg = config.clone();

    if verbose {
        println!("{}", "Scanning workspace...".bright_black());
    }
    let workspace_metadata =
        Engine::scan_workspace(&cfg, tower_lsp::lsp_types::PositionEncodingKind::UTF8, None);

    let mut global_used_fragments = ahash::AHashSet::default();
    for doc in workspace_metadata.documents.values() {
        for spread in &doc.fragment_spreads {
            global_used_fragments.insert(spread.clone());
        }
    }

    let mut global_public_fragments: Vec<FragmentCompletionInfo> = Vec::new();

    for doc in workspace_metadata.documents.values() {
        let package_root = doc.package_root.clone();
        let project_import = cfg
            .get_project_for_path(&doc.uri.to_file_path().unwrap_or_default())
            .and_then(|p| p.import.as_deref().map(Arc::from));

        for frag in doc.fragments() {
            if frag.is_public {
                global_public_fragments.push(FragmentCompletionInfo {
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
                });
            }
        }
    }

    for (project_config, project_meta) in cfg.projects.iter().zip(&workspace_metadata.projects) {
        reporter.report_project_start(&project_config.include.as_key());

        if !execute_project_check(
            &cfg.base_dir,
            &project_config.schema,
            &project_meta.files,
            &workspace_metadata.documents,
            &global_used_fragments,
            &global_public_fragments,
            &config,
            verbose,
            reporter.as_ref(),
        )
        .await
        {
            success = false;
        }
    }

    // Check for duplicate operation names across all projects if the rule is enabled
    if let Some(rules) = &config.rules
        && let Some(true) = rules.unique_operation_name
    {
        for (op_name, projects_map) in &workspace_metadata.operation_names_by_project {
            for (project_idx, paths) in projects_map {
                if paths.len() > 1 {
                    success = false;
                    let project_name = &cfg.projects[*project_idx].include.as_key();
                    let display_paths: Vec<PathBuf> = paths
                        .iter()
                        .map(|path| {
                            path.strip_prefix(&cfg.base_dir)
                                .unwrap_or(path)
                                .to_path_buf()
                        })
                        .collect();
                    let path_refs: Vec<&std::path::Path> =
                        display_paths.iter().map(|p| p.as_path()).collect();
                    reporter.report_duplicate_operation(op_name, project_name, &path_refs);
                }
            }
        }
    }

    if !success {
        reporter.report_failure();
        std::process::exit(1);
    } else {
        reporter.report_success(verbose);
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_project_check(
    base_dir: &std::path::Path,
    source: &SchemaSource,
    project_files: &[PathBuf],
    all_documents: &HashMap<PathBuf, DocumentState>,
    global_used_fragments: &ahash::AHashSet<Arc<str>>,
    global_public_fragments: &[FragmentCompletionInfo],
    config: &Config,
    verbose: bool,
    reporter: &dyn Reporter,
) -> bool {
    let mut texts = Vec::new();
    for file in source.files() {
        match std::fs::read_to_string(base_dir.join(file)) {
            Ok(t) => {
                texts.push(t);
            }
            Err(e) => {
                reporter.report_error(&format!("Failed to read schema {}: {}", source.as_key(), e));
                return false;
            }
        }
    }
    let combined_text = utils::merge_schema_texts(&texts);
    let schema = match Schema::parse(&combined_text, source.as_key()) {
        Ok(s) => s,
        Err(e) => {
            reporter.report_error(&format!(
                "Failed to parse schema {}: {}",
                source.as_key(),
                e
            ));
            return false;
        }
    };
    let valid_schema = match schema.validate() {
        Ok(v) => v,
        Err(e) => {
            reporter.report_error(&format!(
                "Schema validation failed {}: {}",
                source.as_key(),
                e
            ));
            return false;
        }
    };

    let mut found_any = false;

    let mut project_fragments = Vec::new();
    for path in project_files {
        if let Some(doc) = all_documents.get(path) {
            for frag in doc.fragments() {
                project_fragments.push(FragmentCompletionInfo {
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
                });
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
                .any(|f| f.name.as_ref() == pub_frag.name.as_ref() && f.uri == pub_frag.uri)
            {
                available_fragments.push(pub_frag.clone());
            }
        }

        available_fragments.sort_by(|a, b| {
            let a_same_pkg = graphox_core::utils::paths_match(
                a.package_root.as_deref(),
                doc.package_root.as_deref(),
            );
            let b_same_pkg = graphox_core::utils::paths_match(
                b.package_root.as_deref(),
                doc.package_root.as_deref(),
            );
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
            let display_path = path.strip_prefix(base_dir).unwrap_or(path);

            for d in diagnostics {
                let is_issue = matches!(
                    d.severity,
                    Some(DiagnosticSeverity::ERROR) | Some(DiagnosticSeverity::WARNING)
                );

                if is_issue || verbose {
                    if is_issue {
                        found_any = true;
                    }
                    reporter.report_diagnostic(display_path, &d, verbose);
                }
            }
        }
    }

    !found_any
}
