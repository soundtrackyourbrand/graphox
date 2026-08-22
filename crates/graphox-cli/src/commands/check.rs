use crate::reporters::Reporter;
use ahash::AHashMap as HashMap;
use colored::*;
use graphox_core::config::SchemaSource;
use graphox_core::engine::Engine;
use graphox_core::schema;
use graphox_core::{Config, DocumentState};
use graphox_features::completion::FragmentCompletionInfo;
use graphox_features::diagnostics::DocumentDiagnostics;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp_server::ls_types::DiagnosticSeverity;

use super::{ValidSchema, build_validated_schemas};

pub async fn run_check(config: Config, verbose: bool, reporter: Box<dyn Reporter>) {
    let mut success = true;
    let cfg = config.clone();

    if verbose {
        println!("{}", "Scanning workspace...".bright_black());
    }
    // Scan the workspace and validate the (deduplicated) project schemas concurrently:
    // neither depends on the other, and many projects share a schema, so validating
    // each unique schema once — in parallel, overlapped with the scan — replaces what
    // was a per-project load+validate of the same large schemas.
    let (workspace_metadata, validated_schemas) = rayon::join(
        || {
            Engine::scan_workspace(
                &cfg,
                tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
                None,
            )
        },
        || build_validated_schemas(&cfg),
    );

    let mut global_used_fragments = ahash::AHashSet::default();
    for doc in workspace_metadata.documents.values() {
        for spread in doc.fragment_spreads.iter() {
            global_used_fragments.insert(spread.clone());
        }
    }

    let mut global_public_fragments: Vec<FragmentCompletionInfo> = Vec::new();

    for doc in workspace_metadata.documents.values() {
        let package_root = doc.package_root.clone();
        let project_import = cfg
            .get_project_for_path(&graphox_core::utils::uri_to_path(&doc.uri).unwrap_or_default())
            .and_then(|p| p.import().map(Arc::from));

        for frag in doc.fragments.iter() {
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
                    transitive_deps: frag.transitive_deps.clone(),
                    selected_fields: frag.selected_fields.clone(),
                    type_fields: frag.type_fields.clone(),
                    requirements: std::collections::BTreeMap::new(),
                    worst_slo: None,
                });
            }
        }
    }

    for (project_config, project_meta) in cfg.projects().iter().zip(&workspace_metadata.projects) {
        reporter.report_project_start(&project_config.include().as_key());

        let project_files = &project_meta.files;

        // A project whose pattern matches nothing would otherwise pass silently:
        // every stage below iterates the file list, so zero files means zero
        // diagnostics and a clean exit. That is almost always a mistyped
        // `documents` pattern, and it hides every problem in the project. The
        // schema check still runs below, so a project can report both faults.
        if project_files.is_empty() && !cfg.get_project_allow_no_documents(project_config) {
            reporter.report_error(&format!(
                "Project '{}' matched no documents. Fix the pattern, or set \
                 `allow_no_documents: true` to allow it.",
                project_config.include().as_key()
            ));
            success = false;
        }

        if !execute_project_check(
            cfg.base_dir(),
            project_config.schema(),
            &validated_schemas,
            project_files,
            &workspace_metadata.documents,
            &global_used_fragments,
            &global_public_fragments,
            &config,
            project_config,
            verbose,
            reporter.as_ref(),
        )
        .await
        {
            success = false;
        }
    }

    // Check for duplicate operation names across all projects if the rule is enabled
    if config.rules().unique_operation_name() {
        for (op_name, projects_map) in &workspace_metadata.operation_names_by_project {
            for (project_idx, paths) in projects_map {
                if paths.len() > 1 {
                    success = false;
                    let project_name = cfg.projects()[*project_idx].include().as_key();
                    let display_paths: Vec<PathBuf> =
                        paths.iter().map(|path| cfg.relativize(path)).collect();
                    let path_refs: Vec<&std::path::Path> =
                        display_paths.iter().map(|p| p.as_path()).collect();
                    reporter.report_duplicate_operation(op_name, &project_name, &path_refs);
                }
            }
        }
    }

    if !success {
        reporter.report_failure();
        graphox_core::utils::flush_stdio();
        std::process::exit(1);
    } else {
        reporter.report_success(verbose);
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_project_check(
    base_dir: &Path,
    source: &SchemaSource,
    validated_schemas: &HashMap<String, Result<ValidSchema, String>>,
    project_files: &[PathBuf],
    all_documents: &HashMap<PathBuf, DocumentState>,
    global_used_fragments: &ahash::AHashSet<Arc<str>>,
    global_public_fragments: &[FragmentCompletionInfo],
    config: &Config,
    project_config: &graphox_core::config::ProjectConfig,
    verbose: bool,
    reporter: &dyn Reporter,
) -> bool {
    let valid_schema = match validated_schemas.get(&source.as_key()) {
        Some(Ok(schema)) => schema.clone(),
        Some(Err(e)) => {
            reporter.report_error(e);
            return false;
        }
        None => {
            // Defensive fallback: schema not pre-validated (should not happen, since the
            // cache is built from these same projects).
            match schema::load_schema_with_cache(base_dir, source, config.enable_schema_cache()) {
                Ok(s) => Arc::new(s.validate().expect("Schema should be valid")),
                Err(e) => {
                    reporter.report_error(&format!(
                        "Failed to load schema {}: {}",
                        source.as_key(),
                        e
                    ));
                    return false;
                }
            }
        }
    };

    let found_any = std::sync::atomic::AtomicBool::new(false);

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
                    transitive_deps: frag.transitive_deps.clone(),
                    selected_fields: frag.selected_fields.clone(),
                    type_fields: frag.type_fields.clone(),
                    requirements: std::collections::BTreeMap::new(),
                    worst_slo: None,
                });
            }
        }
    }

    // Per-project (not per-document): merge project-specific rules with the global
    // rules once, and build the deduplicated fragment set once. Only the per-document
    // same-package ordering differs, so just that is done in the parallel loop below.
    let effective_config = if let Some(project_rules) = project_config.rules() {
        config
            .clone()
            .with_rules(config.rules().merge(project_rules))
    } else {
        config.clone()
    };

    let mut available_base = project_fragments;
    let mut seen: ahash::AHashSet<(Arc<str>, tower_lsp_server::ls_types::Uri)> = available_base
        .iter()
        .map(|f| (f.name.clone(), f.uri.clone()))
        .collect();
    for pub_frag in global_public_fragments {
        if seen.insert((pub_frag.name.clone(), pub_frag.uri.clone())) {
            available_base.push(pub_frag.clone());
        }
    }

    project_files.par_iter().for_each(|path| {
        let Some(doc) = all_documents.get(path) else {
            return;
        };
        let mut available_fragments = available_base.clone();

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
            Some(&effective_config),
            verbose,
            true,
        );
        if !diagnostics.is_empty() {
            let display_path = config.relativize(path);

            for d in diagnostics {
                let is_issue = matches!(
                    d.severity,
                    Some(DiagnosticSeverity::ERROR) | Some(DiagnosticSeverity::WARNING)
                );

                if is_issue || verbose {
                    if is_issue {
                        found_any.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    reporter.report_diagnostic(&display_path, &d, verbose);
                }
            }
        }
    });

    !found_any.load(std::sync::atomic::Ordering::Relaxed)
}
