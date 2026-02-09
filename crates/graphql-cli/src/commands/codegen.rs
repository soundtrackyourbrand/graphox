use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use colored::*;
use graphql_codegen as codegen;
use graphql_core::DocumentState;
use graphql_core::config::{Config, SchemaSource};
use graphql_core::engine::{Engine, FragmentMetadata, ProjectContext};
use graphql_core::schema;
use graphql_core::schema_cache;
use graphql_core::utils;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

pub struct CodegenParams<'a> {
    pub base_dir: &'a Path,
    pub source: &'a SchemaSource,
    pub project_files: &'a [PathBuf],
    pub output_dir: Option<&'a str>,
    pub scalars: &'a Option<HashMap<String, String>>,
    pub schema_import: &'a Option<String>,
    pub project_context: &'a ProjectContext,
    pub global_metadata: &'a [FragmentMetadata],
    pub generate_ast_for_fragments: bool,
    pub workspace_documents: &'a HashMap<PathBuf, DocumentState>,
    pub generate_permissions: bool,
    pub document_suffix: &'a str,
    pub variables_suffix: &'a str,
    pub fragment_suffix: &'a str,
}

pub async fn run_codegen(
    mut config: Config,
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

    'watch_loop: loop {
        println!("{}", "Watching for changes...".bright_black());
        let _ = execute_codegen(config.clone(), output_dir, verbose, false).await;

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let (config_tx, mut config_rx) = tokio::sync::mpsc::channel(1);

        let gitignore = utils::get_gitignore_matcher(&config.base_dir);
        let mut output_dirs = Vec::new();
        if let Some(out) = &config.output_dir {
            output_dirs.push(config.base_dir.join(out));
        }
        for p in &config.projects {
            if let Some(out) = &p.output_dir {
                output_dirs.push(config.base_dir.join(out));
            }
        }

        let config_tx_clone = config_tx.clone();
        let base_dir_for_watcher = config.base_dir.clone();
        let debounce_ms = config.codegen_watch_debounce_ms();
        let mut debouncer = notify_debouncer_mini::new_debouncer(
            std::time::Duration::from_millis(debounce_ms),
            move |res: notify_debouncer_mini::DebounceEventResult| match res {
                Ok(events) => {
                    let has_config_change = events.iter().any(|e| {
                        let file_name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        file_name == "graphql.yaml" || file_name == "graphql.yml"
                    });

                    if has_config_change {
                        let _ = config_tx_clone.blocking_send(());
                        return;
                    }

                    let has_relevant_change = events.iter().any(|e| {
                        if !utils::is_relevant_file(&e.path) {
                            return false;
                        }
                        if utils::is_path_ignored(&e.path, &gitignore) {
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

        let config_yaml = base_dir_for_watcher.join("graphql.yaml");
        let config_yml = base_dir_for_watcher.join("graphql.yml");
        if config_yaml.exists() {
            debouncer
                .watcher()
                .watch(&config_yaml, notify::RecursiveMode::NonRecursive)
                .ok();
        }
        if config_yml.exists() {
            debouncer
                .watcher()
                .watch(&config_yml, notify::RecursiveMode::NonRecursive)
                .ok();
        }

        for project in &config.projects {
            for pattern in project.include.patterns() {
                let watch_path = config.base_dir.join(utils::get_glob_root(&pattern));
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

        loop {
            tokio::select! {
                _ = config_rx.recv() => {
                    println!("{}", "\nConfiguration file changed, reloading...".bright_yellow());

                    if let Some(new_config) = Config::load_from_dir(&config.base_dir) {
                        println!("{}", "Configuration reloaded successfully".bright_green());
                        config = new_config;
                        continue 'watch_loop;
                    } else {
                        eprintln!("{}", "Failed to reload configuration, continuing with old config".red());
                    }
                }
                _ = rx.recv() => {
                    println!(
                        "{}",
                        "\nChange detected, re-running codegen...".bright_black()
                    );
                    let _ = execute_codegen(config.clone(), output_dir, verbose, false).await;
                }
            }
        }
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

    if clean {
        if let Err(e) = schema_cache::clear_cache() {
            eprintln!("{}: {}", "Failed to clear schema cache".red(), e);
            success = false;
        } else if verbose {
            println!("{}", "Cleared schema cache".bright_black());
        }
    }

    let workspace_metadata = Engine::scan_workspace(&cfg);
    let global_metadata = &workspace_metadata.fragments;

    let global_output_dir = output_dir.or(cfg.output_dir.as_deref());
    for (project, project_meta) in cfg.projects.iter().zip(&workspace_metadata.projects) {
        if !project.codegen_enabled() {
            if verbose {
                println!(
                    "{}: {} (codegen disabled)",
                    "Skipping project".bright_black(),
                    project.include.as_key().bright_black()
                );
            }
            continue;
        }

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

        let valid_schema = match schema::load_and_validate_schema(&cfg.base_dir, &project.schema) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}", e.to_string().red());
                success = false;
                continue;
            }
        };

        let project_context =
            Engine::resolve_project_context(&valid_schema, global_metadata, project_files);

        if !clean
            && project.generate_permissions.unwrap_or(false)
            && let Some(out_dir) = project_output_dir
        {
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
            let content =
                codegen::generate_permissions_content(&valid_schema, &cfg.scalars, &schema_import);
            if let Err(e) = std::fs::write(&permissions_path, content) {
                eprintln!("{}: {}", "Failed to write permissions".red(), e);
                success = false;
            }
        }

        let document_suffix = project
            .document_suffix
            .as_deref()
            .or(cfg.document_suffix.as_deref())
            .unwrap_or("Document");
        let variables_suffix = project
            .variables_suffix
            .as_deref()
            .or(cfg.variables_suffix.as_deref())
            .unwrap_or("Variables");
        let fragment_suffix = project
            .fragment_suffix
            .as_deref()
            .or(cfg.fragment_suffix.as_deref())
            .unwrap_or("");

        match execute_project_codegen_entry(
            CodegenParams {
                base_dir: &cfg.base_dir,
                source: &project.schema,
                project_files,
                output_dir: project_output_dir,
                scalars: &cfg.scalars,
                schema_import: &schema_import,
                project_context: &project_context,
                global_metadata,
                generate_ast_for_fragments: cfg.generate_ast_for_fragments.unwrap_or(false),
                workspace_documents: &workspace_metadata.documents,
                generate_permissions: project.generate_permissions.unwrap_or(false),
                document_suffix,
                variables_suffix,
                fragment_suffix,
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

    if !clean && let Some(out_dir) = global_output_dir {
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
            let content = codegen::generate_entrypoint_content(
                &out_dir_path,
                &all_generated_operations,
                cfg.document_suffix(),
                cfg.variables_suffix(),
            );
            std::fs::create_dir_all(&out_dir_path).ok();
            if let Err(e) = std::fs::write(&entrypoint_path, content) {
                eprintln!("{}: {}", "Failed to write entrypoint".red(), e);
                success = false;
            }

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
                    let path_no_ext = if path_str.ends_with(".ts") {
                        &path_str[..path_str.len() - 3]
                    } else {
                        &path_str
                    };

                    sonic_rs::json!({
                        "source": op.source_text,
                        "path": path_no_ext,
                        "name": format!("{}Document", op.operation_type_name)
                    })
                })
                .collect();

            let manifest_json = sonic_rs::to_string_pretty(&manifest_entries).unwrap();
            std::fs::create_dir_all(&out_dir_path).ok();
            if let Err(e) = std::fs::write(&manifest_path, manifest_json) {
                eprintln!("{}: {}", "Failed to write manifest".red(), e);
                success = false;
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
    let valid_schema = match schema::load_and_validate_schema(base_dir, source) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e.to_string().red());
            return false;
        }
    };

    let ts_code = codegen::generate_schema_types(&valid_schema, scalars);
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
) -> Result<Vec<codegen::OperationGenerated>, ()> {
    if !clean {
        generate_project_files(params, verbose).await
    } else {
        clean_project_files(params, verbose).await
    }
}

async fn generate_project_files(
    params: CodegenParams<'_>,
    verbose: bool,
) -> Result<Vec<codegen::OperationGenerated>, ()> {
    let valid_schema = match schema::load_and_validate_schema(params.base_dir, params.source) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e.to_string().red());
            return Err(());
        }
    };

    let shared_type_cache = codegen::TypeCache::new();

    let results: Vec<_> = params
        .project_files
        .par_iter()
        .filter_map(|path| params.workspace_documents.get(path).map(|doc| (path, doc)))
        .filter(|(_, doc)| !doc.get_graphql_trees().is_empty())
        .map(|(path, doc)| {
            let ctx = codegen::CodegenContext::new(
                &valid_schema,
                &params.project_context.fragment_to_path,
                &params.project_context.fragment_to_import,
                &params.project_context.fragment_to_type_only,
                &params.project_context.all_fragments,
                path,
                params.scalars,
                params.schema_import,
                params.generate_ast_for_fragments,
                &params.project_context.fragment_dependencies,
                &shared_type_cache,
                params.document_suffix,
                params.variables_suffix,
                params.fragment_suffix,
            );

            execute_single_file_codegen(doc, &ctx, params.output_dir, params.base_dir, verbose)
                .map_err(|e| (path.to_path_buf(), e))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|res| match res {
            Ok(ops) => Ok(ops),
            Err((path, e)) => {
                if !e.contains("No executable operations") {
                    eprintln!(
                        "{} {}: {}",
                        "Error generating types for".red(),
                        path.display().to_string().red(),
                        e.red()
                    );
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
                                        "Hint".yellow(),
                                        meta.name.blue(),
                                        meta.path.blue()
                                    );
                                } else if !meta.is_public {
                                    eprintln!(
                                        "  {}: Fragment '{}' exists in {} but is not marked as @public",
                                        "Hint".yellow(),
                                        meta.name.blue(),
                                        meta.path.blue()
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
}

async fn clean_project_files(
    params: CodegenParams<'_>,
    verbose: bool,
) -> Result<Vec<codegen::OperationGenerated>, ()> {
    let success = params
        .project_files
        .par_iter()
        .map(|path| {
            let out_path = utils::get_output_path(path, params.base_dir, params.output_dir);
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

fn execute_single_file_codegen(
    doc: &DocumentState,
    ctx: &codegen::CodegenContext<'_>,
    output_dir: Option<&str>,
    base_dir: &Path,
    verbose: bool,
) -> Result<Vec<codegen::OperationGenerated>, String> {
    let (ts_code, mut ops) = codegen::generate_typescript(doc, ctx)?;
    let out_path_raw = utils::get_output_path(
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
