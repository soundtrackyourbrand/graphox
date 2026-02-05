use colored::*;
use fnv::{FnvHashMap as HashMap, FnvHashSet as HashSet};
use graphql_rust::config::{Config, SchemaSource};
use graphql_rust::engine::{Engine, FragmentMetadata, ProjectContext};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

struct CodegenParams<'a> {
    base_dir: &'a Path,
    source: &'a SchemaSource,
    project_files: &'a [PathBuf],
    output_dir: Option<&'a str>,
    scalars: &'a Option<HashMap<String, String>>,
    schema_import: &'a Option<String>,
    project_context: &'a ProjectContext,
    global_metadata: &'a [FragmentMetadata],
    generate_ast_for_fragments: bool,
    workspace_documents: &'a HashMap<PathBuf, graphql_rust::DocumentState>,
    generate_permissions: bool,
}

pub async fn run_codegen(
    config: Config,
    output_dir: Option<&str>,
    watch: bool,
    verbose: bool,
    clean: bool,
) {
    if !watch {
        if !execute_codegen(config, output_dir, verbose, clean).await {
            std::process::exit(1);
        }
        return;
    }

    println!("{}", "Watching for changes...".bright_black());
    let _ = execute_codegen(config.clone(), output_dir, verbose, false).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    let gitignore = graphql_rust::utils::get_gitignore_matcher(&config.base_dir);
    let mut output_dirs = Vec::new();
    if let Some(out) = &config.output_dir {
        output_dirs.push(config.base_dir.join(out));
    }
    for p in &config.projects {
        if let Some(out) = &p.output_dir {
            output_dirs.push(config.base_dir.join(out));
        }
    }

    let mut debouncer = notify_debouncer_mini::new_debouncer(
        std::time::Duration::from_millis(200),
        move |res: notify_debouncer_mini::DebounceEventResult| match res {
            Ok(events) => {
                let has_relevant_change = events.iter().any(|e| {
                    if !graphql_rust::utils::is_relevant_file(&e.path) {
                        return false;
                    }
                    if graphql_rust::utils::is_path_ignored(&e.path, &gitignore) {
                        return false;
                    }
                    if output_dirs.iter().any(|d| e.path.starts_with(d)) {
                        return false;
                    }
                    true
                });
                if has_relevant_change {
                    let _ = tx.blocking_send(());
                }
            }
            Err(e) => eprintln!("{}: {:?}", "Watch error".red(), e),
        },
    )
    .expect("Failed to create debouncer");

    // Watch project include directories
    for project in &config.projects {
        for pattern in project.include.patterns() {
            // This is a simplification, ideally we'd find the common parent
            let path = config.base_dir.join(pattern);
            let watch_path = if path.to_string_lossy().contains('*') {
                // Find first non-glob parent
                let mut p = path.clone();
                while p.to_string_lossy().contains('*') {
                    if !p.pop() {
                        break;
                    }
                }
                p
            } else {
                path
            };
            debouncer
                .watcher()
                .watch(&watch_path, notify::RecursiveMode::Recursive)
                .ok();
        }
    }

    for project in &config.projects {
        for file in project.schema.files() {
            debouncer
                .watcher()
                .watch(
                    &config.base_dir.join(file),
                    notify::RecursiveMode::NonRecursive,
                )
                .ok();
        }
    }
    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            for file in st.schema.files() {
                debouncer
                    .watcher()
                    .watch(
                        &config.base_dir.join(file),
                        notify::RecursiveMode::NonRecursive,
                    )
                    .ok();
            }
        }
    }

    while rx.recv().await.is_some() {
        println!(
            "{}",
            "\nChange detected, re-running codegen...".bright_black()
        );
        let _ = execute_codegen(config.clone(), output_dir, verbose, false).await;
    }
}

async fn execute_codegen(
    config: Config,
    output_dir: Option<&str>,
    verbose: bool,
    clean: bool,
) -> bool {
    let mut success = true;
    let mut all_generated_operations = Vec::new();

    let cfg = config;

    let workspace_metadata = Engine::scan_workspace(&cfg, |_, _| {});
    let global_metadata = &workspace_metadata.fragments;

    let global_output_dir = output_dir.or(cfg.output_dir.as_deref());
    for (project, project_meta) in cfg.projects.iter().zip(&workspace_metadata.projects) {
        let project_files = &project_meta.files;

        println!("Processing project: {}", project.include.as_key().blue());
        let project_output_dir = project.output_dir.as_deref().or(global_output_dir);

        let project_schema_files: HashSet<_> = project.schema.files().into_iter().collect();

        let schema_import = cfg.schema_types.as_ref().and_then(|sts| {
            let mut matches: Vec<_> = sts
                .iter()
                .filter(|st| {
                    let st_files = st.schema.files();
                    st_files.iter().all(|f| project_schema_files.contains(f))
                })
                .collect();

            matches.sort_by_key(|st| std::cmp::Reverse(st.schema.files().len()));
            matches.first().and_then(|st| st.import.clone())
        });

        let valid_schema = match graphql_rust::schema::load_and_validate_schema(&cfg.base_dir, &project.schema) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}", e.to_string().red());
                success = false;
                continue;
            }
        };

        let project_context =
            Engine::resolve_project_context(&valid_schema, global_metadata, project_files);

        if !clean && project.generate_permissions.unwrap_or(false) {
            if let Some(out_dir) = project_output_dir {
                let out_dir_path = cfg.base_dir.join(out_dir);
                std::fs::create_dir_all(&out_dir_path).ok();
                let permissions_path = out_dir_path.join("permissions.ts");
                if verbose {
                    println!(
                        "{}: {}",
                        "Generating permissions".bright_black(),
                        permissions_path.display().to_string().bright_black()
                    );
                }
                let content = graphql_rust::features::codegen::generate_permissions_content(
                    &valid_schema,
                    &cfg.scalars,
                    &schema_import,
                );
                if let Err(e) = std::fs::write(&permissions_path, content) {
                    eprintln!("{}: {}", "Failed to write permissions".red(), e);
                    success = false;
                }
            }
        }

        match execute_project_codegen_entry(
            CodegenParams {
                base_dir: &cfg.base_dir,
                source: &project.schema,
                project_files,
                output_dir: project_output_dir,
                scalars: &cfg.scalars,
                schema_import: &schema_import,
                project_context: &project_context,
                global_metadata: &global_metadata,
                generate_ast_for_fragments: cfg.generate_ast_for_fragments.unwrap_or(false),
                workspace_documents: &workspace_metadata.documents,
                generate_permissions: project.generate_permissions.unwrap_or(false),
            },
            verbose,
            clean,
        )
        .await
        {
            Ok(ops) => {
                all_generated_operations.extend(ops);
            }
            Err(_) => success = false,
        }
    }

    if let Some(schema_types) = &cfg.schema_types {
        for st in schema_types {
            let abs_output = cfg.base_dir.join(&st.output);
            if clean {
                if abs_output.exists() {
                    if let Err(e) = std::fs::remove_file(&abs_output) {
                        eprintln!(
                            "{}: {} - {}",
                            "Failed to remove".red(),
                            abs_output.display().to_string().red(),
                            e
                        );
                        success = false;
                    } else if verbose {
                        println!(
                            "{}: {}",
                            "Removed".bright_black(),
                            abs_output.display().to_string().bright_black()
                        );
                    }
                }
            } else {
                println!("Generating types for schema: {}", st.output.blue());
                if !execute_schema_codegen(
                    &cfg.base_dir,
                    &st.schema,
                    &abs_output.to_string_lossy(),
                    &cfg.scalars,
                    verbose,
                )
                .await
                {
                    success = false;
                }
            }
        }
    }

    if !clean {
        if let Some(out_dir) = global_output_dir {
            let out_dir_path = cfg.base_dir.join(out_dir);
            let entrypoint_path = out_dir_path.join("graphql.ts");
            if !all_generated_operations.is_empty() {
                if verbose {
                    println!(
                        "{}: {}",
                        "Generating entrypoint".bright_black(),
                        entrypoint_path.display().to_string().bright_black()
                    );
                }
                let content = graphql_rust::features::codegen::generate_entrypoint_content(
                    &out_dir_path,
                    &all_generated_operations,
                );
                if let Err(e) = std::fs::write(&entrypoint_path, content) {
                    eprintln!("{}: {}", "Failed to write entrypoint".red(), e);
                    success = false;
                }

                // Generate manifest for SWC plugin
                let manifest_path = out_dir_path.join("manifest.json");
                if verbose {
                    println!(
                        "{}: {}",
                        "Generating manifest".bright_black(),
                        manifest_path.display().to_string().bright_black()
                    );
                }
                let manifest_entries: Vec<_> = all_generated_operations
                    .iter()
                    .map(|op| {
                        let rel_path = pathdiff::diff_paths(&op.codegen_path, &out_dir_path)
                            .unwrap_or_else(|| op.codegen_path.clone());
                        let mut path_str = rel_path.to_string_lossy().to_string();
                        if !path_str.starts_with('.') && !path_str.starts_with('/') {
                            path_str = format!("./{}", path_str);
                        }
                        // Remove .ts extension
                        let path_no_ext = if path_str.ends_with(".ts") {
                            &path_str[..path_str.len() - 3]
                        } else {
                            &path_str
                        };

                        serde_json::json!({
                            "source": op.source_text,
                            "path": path_no_ext,
                            "name": format!("{}Document", op.operation_type_name)
                        })
                    })
                    .collect();

                let manifest_json = serde_json::to_string_pretty(&manifest_entries).unwrap();
                if let Err(e) = std::fs::write(&manifest_path, manifest_json) {
                    eprintln!("{}: {}", "Failed to write manifest".red(), e);
                    success = false;
                }
            }
        }
    }

    success
}

async fn execute_schema_codegen(
    base_dir: &Path,
    source: &SchemaSource,
    output_path: &str,
    scalars: &Option<HashMap<String, String>>,
    verbose: bool,
) -> bool {
    let valid_schema = match graphql_rust::schema::load_and_validate_schema(base_dir, source) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e.to_string().red());
            return false;
        }
    };

    let ts_code = graphql_rust::features::codegen::generate_schema_types(&valid_schema, scalars);
    let out_path = Path::new(output_path);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Err(e) = std::fs::write(out_path, ts_code) {
        eprintln!(
            "{} {}: {}",
            "Failed to write schema types file".red(),
            output_path.red(),
            e
        );
        return false;
    }
    if verbose {
        println!(
            "{}: {}",
            "Generated".bright_black(),
            out_path.display().to_string().bright_black()
        );
    }
    true
}

async fn execute_project_codegen_entry(
    params: CodegenParams<'_>,
    verbose: bool,
    clean: bool,
) -> Result<Vec<graphql_rust::features::codegen::OperationGenerated>, ()> {
    if !clean {
        let valid_schema = match graphql_rust::schema::load_and_validate_schema(params.base_dir, params.source) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}", e.to_string().red());
                return Err(());
            }
        };

        let results: Vec<_> = params
            .project_files
            .par_iter()
            .filter_map(|path| {
                params.workspace_documents.get(path).map(|doc| (path, doc))
            })
            .filter(|(_, doc)| !doc.get_graphql_trees().is_empty())
            .map(|(path, doc)| {
                let ctx = graphql_rust::features::codegen::CodegenContext {
                    schema: &valid_schema,
                    fragment_to_path: &params.project_context.fragment_to_path,
                    fragment_to_import: &params.project_context.fragment_to_import,
                    fragment_to_type_only: &params.project_context.fragment_to_type_only,
                    all_fragments: &params.project_context.all_fragments,
                    current_file_path: path,
                    scalars: params.scalars,
                    schema_import: params.schema_import,
                    generate_ast_for_fragments: params.generate_ast_for_fragments,
                };

                execute_single_file_codegen(doc, &ctx, params.output_dir, params.base_dir, verbose)
                    .map_err(|e| (path.to_path_buf(), e))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|res| match res {
                Ok(ops) => Ok(ops),
                Err((path, e)) => {
                    if !e.contains("No executable operations") {
                        eprintln!("{} {}: {}", "Error generating types for".red(), path.display().to_string().red(), e.red());
                        if e.contains("Fragment") && e.contains("not found") {
                            for meta in params.global_metadata {
                                if e.contains(&format!("'{}'", meta.name)) {
                                     let is_local = params.project_files.iter().any(|pf: &PathBuf| {
                                         std::fs::canonicalize(pf).unwrap_or_else(|_| pf.clone()) == 
                                         std::fs::canonicalize(&meta.path).unwrap_or_else(|_| PathBuf::from(&meta.path))
                                     });

                                     if is_local {
                                         eprintln!(
                                             "  {}: Fragment '{}' exists in {} but association might have failed.",
                                             "Hint".yellow(), meta.name.blue(), meta.path.blue()
                                         );
                                     } else if !meta.is_public {
                                         eprintln!(
                                             "  {}: Fragment '{}' exists in {} but is not marked as @public",
                                             "Hint".yellow(), meta.name.blue(), meta.path.blue()
                                         );
                                     }
                                }
                            }
                        }
                        Err(())
                    } else {
                        Ok(Vec::new())
                    }
                }
            })
            .collect();

        let mut all_ops = Vec::new();
        let mut success = true;
        for res in results {
            match res {
                Ok(ops) => all_ops.extend(ops),
                Err(_) => success = false,
            }
        }

        if success { Ok(all_ops) } else { Err(()) }
    } else {
        // Clean mode
        let success = params
            .project_files
            .par_iter()
            .map(|path| {
                let out_path =
                    graphql_rust::utils::get_output_path(path, params.base_dir, params.output_dir);
                let mut ok = true;
                if out_path.exists() {
                    if let Err(e) = std::fs::remove_file(&out_path) {
                        eprintln!(
                            "{}: {} - {}",
                            "Failed to remove".red(),
                            out_path.display().to_string().red(),
                            e
                        );
                        ok = false;
                    } else if verbose {
                        println!(
                            "{}: {}",
                            "Removed".bright_black(),
                            out_path.display().to_string().bright_black()
                        );
                    }
                }
                ok
            })
            .reduce(|| true, |a, b| a && b);

        let mut entrypoint_ok = true;
        let mut manifest_ok = true;
        let mut permissions_ok = true;
        if let Some(out_dir) = params.output_dir {
            let entrypoint_path = params.base_dir.join(out_dir).join("graphql.ts");
            if entrypoint_path.exists() {
                if let Err(e) = std::fs::remove_file(&entrypoint_path) {
                    eprintln!(
                        "{} {}: {}",
                        "Failed to remove entrypoint".red(),
                        entrypoint_path.display().to_string().red(),
                        e
                    );
                    entrypoint_ok = false;
                } else if verbose {
                    println!(
                        "{}: {}",
                        "Removed".bright_black(),
                        entrypoint_path.display().to_string().bright_black()
                    );
                }
            }

            let manifest_path = params.base_dir.join(out_dir).join("manifest.json");
            if manifest_path.exists() {
                if let Err(e) = std::fs::remove_file(&manifest_path) {
                    eprintln!(
                        "{} {}: {}",
                        "Failed to remove manifest".red(),
                        manifest_path.display().to_string().red(),
                        e
                    );
                    manifest_ok = false;
                } else if verbose {
                    println!(
                        "{}: {}",
                        "Removed".bright_black(),
                        manifest_path.display().to_string().bright_black()
                    );
                }
            }

            if params.generate_permissions {
                let permissions_path = params.base_dir.join(out_dir).join("permissions.ts");
                if permissions_path.exists() {
                    if let Err(e) = std::fs::remove_file(&permissions_path) {
                        eprintln!(
                            "{} {}: {}",
                            "Failed to remove permissions".red(),
                            permissions_path.display().to_string().red(),
                            e
                        );
                        permissions_ok = false;
                    } else if verbose {
                        println!(
                            "{}: {}",
                            "Removed".bright_black(),
                            permissions_path.display().to_string().bright_black()
                        );
                    }
                }
            }
        }

        if success && entrypoint_ok && manifest_ok && permissions_ok {
            Ok(Vec::new())
        } else {
            Err(())
        }
    }
}

fn execute_single_file_codegen(
    doc: &graphql_rust::DocumentState,
    ctx: &graphql_rust::features::codegen::CodegenContext<'_>,
    output_dir: Option<&str>,
    base_dir: &Path,
    verbose: bool,
) -> Result<Vec<graphql_rust::features::codegen::OperationGenerated>, String> {
    let (ts_code, mut ops) = graphql_rust::features::codegen::generate_typescript(doc, ctx)?;
    let out_path_raw = graphql_rust::utils::get_output_path(
        doc.uri.to_file_path().unwrap().as_path(),
        base_dir,
        output_dir,
    );

    let abs_out_path = if out_path_raw.is_absolute() {
        out_path_raw
    } else {
        base_dir.join(out_path_raw)
    };

    for op in &mut ops {
        op.codegen_path = abs_out_path.clone();
    }

    if let Some(parent) = abs_out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&abs_out_path, ts_code).map_err(|e| e.to_string())?;
    if verbose {
        println!(
            "{}: {}",
            "Generated".bright_black(),
            abs_out_path.display().to_string().bright_black()
        );
    }
    Ok(ops)
}
