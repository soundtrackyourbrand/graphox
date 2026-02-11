use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use colored::*;
use graphox_codegen as codegen;
use graphox_core::DocumentState;
use graphox_core::config::{Config, GlobPattern, SchemaSource};
use graphox_core::engine::{Engine, FragmentMetadata, ProjectContext};
use graphox_core::schema;
use graphox_core::schema_cache;
use graphox_core::utils;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

pub struct CodegenParams<'a> {
    pub base_dir: &'a Path,
    pub source: &'a SchemaSource,
    pub include: &'a GlobPattern,
    pub project_files: &'a [PathBuf],
    pub output_dir: Option<&'a str>,
    pub scalars: &'a Option<HashMap<String, String>>,
    pub schema_import: &'a Option<String>,
    pub type_imports: &'a HashMap<String, String>,
    pub project_context: &'a ProjectContext,
    pub global_metadata: &'a [FragmentMetadata],
    pub generate_ast_for_fragments: bool,
    pub workspace_documents: &'a HashMap<PathBuf, DocumentState>,
    pub emit_permission_data: bool,
    pub document_suffix: &'a str,
    pub variables_suffix: &'a str,
    pub fragment_suffix: &'a str,
    pub fragment_masking: codegen::FragmentMasking,
}

pub async fn run_codegen(mut config: Config, watch: bool, verbose: bool, clean: bool) {
    if !watch {
        if !execute_codegen(config, verbose, clean).await {
            std::process::exit(1);
        }
        return;
    }

    'watch_loop: loop {
        println!("{}", "Watching for changes...".bright_black());
        let _ = execute_codegen(config.clone(), verbose, false).await;

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let (config_tx, mut config_rx) = tokio::sync::mpsc::channel(1);

        let gitignore = utils::get_gitignore_matcher(&config.base_dir);
        let mut output_dirs = Vec::new();
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
                        file_name == "graphox.yaml" || file_name == "graphox.yml"
                    });

                    if has_config_change {
                        let _ = config_tx_clone.try_send(());
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
                        let _ = tx.try_send(());
                    }
                }
                Err(e) => eprintln!("{}: {:?}", "Watch error".red(), e),
            },
        )
        .expect("Failed to create debouncer");

        let config_yaml = base_dir_for_watcher.join("graphox.yaml");
        let config_yml = base_dir_for_watcher.join("graphox.yml");
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

                    if let Ok(Some(new_config)) = Config::load_from_dir(&config.base_dir) {
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
                    let _ = execute_codegen(config.clone(), verbose, false).await;
                }
            }
        }
    }
}

async fn execute_codegen(config: Config, verbose: bool, clean: bool) -> bool {
    let mut success = true;
    let mut project_operations: HashMap<usize, Vec<codegen::OperationGenerated>> = HashMap::new();

    let cfg = config;

    if clean {
        if let Err(e) = schema_cache::clear_cache() {
            eprintln!("{}: {}", "Failed to clear schema cache".red(), e);
            success = false;
        } else if verbose {
            println!("{}", "Cleared schema cache".bright_black());
        }
    }

    let workspace_metadata =
        Engine::scan_workspace(&cfg, tower_lsp::lsp_types::PositionEncodingKind::UTF8, None);
    let global_metadata = &workspace_metadata.fragments;

    for (project_index, (project, project_meta)) in cfg
        .projects
        .iter()
        .zip(&workspace_metadata.projects)
        .enumerate()
    {
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
        let project_output_dir = project.output_dir.as_deref();

        let project_schema_files: HashSet<_> = project.schema.files().into_iter().collect();

        let mut type_imports = HashMap::default();
        let mut schema_import = None;

        if let Some(schema_types) = &cfg.schema_types {
            // 1. Collect all matching schema_types and their import paths
            let mut matches: Vec<_> = schema_types
                .iter()
                .filter(|st| {
                    let st_files = st.schema.files();
                    // Check if this schema_type is a subset of the project schema
                    st_files.iter().all(|f| project_schema_files.contains(f))
                })
                .collect();

            // Sort by number of files descending for specificity
            matches.sort_by_key(|st| std::cmp::Reverse(st.schema.files().len()));

            // 2. Build the type_imports map
            for st in matches.iter().rev() {
                if let Some(import_path) = &st.import
                    && let Ok(st_schema) =
                        schema::load_and_validate_schema(&cfg.base_dir, &st.schema)
                {
                    for type_name in st_schema.types.keys() {
                        type_imports.insert(type_name.to_string(), import_path.clone());
                    }
                }
            }

            // 3. Keep schema_import for backward compatibility (the "best" match)
            schema_import = matches.first().and_then(|st| st.import.clone());
        }

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

        if !clean && project.emit_permission_data.unwrap_or(false) {
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
                let content = codegen::emit_permission_data_content(
                    &valid_schema,
                    &cfg.scalars,
                    &schema_import,
                );
                if let Err(e) = std::fs::write(&permissions_path, content) {
                    eprintln!("{}: {}", "Failed to write permissions".red(), e);
                    success = false;
                }
            } else {
                eprintln!(
                    "{}: emit_permission_data is enabled but no output_dir is specified for project.",
                    "Warning".yellow()
                );
            }
        }

        if !clean && project.generate_possible_types.unwrap_or(false) {
            if let Some(pt_output) = &project.possible_types_output {
                let pt_path = cfg.base_dir.join(pt_output);
                if verbose {
                    println!(
                        "{}: {}",
                        "Generating possibleTypes".bright_black(),
                        pt_path.display().to_string().bright_black()
                    );
                }
                let content = codegen::generate_possible_types(&valid_schema);
                if let Err(e) = std::fs::write(&pt_path, content) {
                    eprintln!("{}: {}", "Failed to write possibleTypes".red(), e);
                    success = false;
                }
            } else {
                eprintln!(
                    "{}: generate_possible_types is enabled but no possible_types_output is specified for project.",
                    "Warning".yellow()
                );
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
                include: &project.include,
                project_files,
                output_dir: project_output_dir,
                scalars: &cfg.scalars,
                schema_import: &schema_import,
                type_imports: &type_imports,
                project_context: &project_context,
                global_metadata,
                generate_ast_for_fragments: cfg.generate_ast_for_fragments.unwrap_or(false),
                workspace_documents: &workspace_metadata.documents,
                emit_permission_data: project.emit_permission_data.unwrap_or(false),
                document_suffix,
                variables_suffix,
                fragment_suffix,
                fragment_masking: codegen::FragmentMasking::from_config(
                    &project
                        .fragment_masking
                        .clone()
                        .or(cfg.fragment_masking.clone()),
                ),
            },
            verbose,
            clean,
        )
        .await
        {
            Ok(ops) => {
                project_operations.insert(project_index, ops);
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

    // Generate graphql.ts and manifest.json for each project
    if !clean {
        for (project_idx, project) in cfg.projects.iter().enumerate() {
            let out_dir = project.output_dir.as_deref().unwrap_or("__generated__");
            let Some(ops) = project_operations.get(&project_idx) else {
                continue;
            };

            let out_dir_path = cfg.base_dir.join(out_dir);

            // Check if path exists but is a file (blocks directory creation)
            if out_dir_path.exists() && out_dir_path.is_file() {
                eprintln!(
                    "{}: output_dir '{}' exists as a file, not a directory",
                    "Error".red(),
                    out_dir_path.display()
                );
                success = false;
                continue;
            }

            let entrypoint_path = out_dir_path.join("graphql.ts");
            let fragment_masking = codegen::FragmentMasking::from_config(
                &project
                    .fragment_masking
                    .clone()
                    .or(cfg.fragment_masking.clone()),
            );

            if verbose {
                println!(
                    "{}: {}",
                    "Generating entrypoint".bright_black(),
                    entrypoint_path.display().to_string().bright_black()
                );
            }
            let content = codegen::generate_entrypoint_content(
                &out_dir_path,
                ops,
                cfg.document_suffix(),
                cfg.variables_suffix(),
                &fragment_masking,
            );
            if let Err(e) = std::fs::create_dir_all(&out_dir_path) {
                eprintln!(
                    "{}: Failed to create directory '{}' - {}",
                    "Error".red(),
                    out_dir_path.display(),
                    e
                );
                success = false;
                continue;
            }
            if let Err(e) = std::fs::write(&entrypoint_path, content) {
                eprintln!(
                    "{}: {} (entrypoint: {}, dir exists: {})",
                    "Failed to write entrypoint".red(),
                    e,
                    entrypoint_path.display(),
                    out_dir_path.exists()
                );
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
            let manifest_entries: Vec<_> = ops
                .iter()
                .map(|op| {
                    let rel_path = pathdiff::diff_paths(&op.codegen_path, &out_dir_path)
                        .unwrap_or_else(|| op.codegen_path.clone());
                    let mut path_str = utils::to_posix_path(&rel_path);
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
            if let Err(e) = std::fs::write(&manifest_path, manifest_json) {
                eprintln!(
                    "{}: {} (manifest: {}, dir exists: {})",
                    "Failed to write manifest".red(),
                    e,
                    manifest_path.display(),
                    out_dir_path.exists()
                );
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
            let glob_pattern = params.include.as_key();
            let include_prefix_path = utils::get_glob_root(&glob_pattern);
            let include_prefix = include_prefix_path.to_str().unwrap_or("");

            let out_path_raw = utils::get_output_path(
                path,
                params.base_dir,
                params.output_dir,
                Some(include_prefix),
            );

            let abs_out_path = if out_path_raw.is_absolute() {
                out_path_raw
            } else {
                params.base_dir.join(out_path_raw)
            };

            let masking_import_path = if let Some(out_dir) = params.output_dir {
                let abs_out_dir = params.base_dir.join(out_dir);
                let abs_file_out_dir = abs_out_path.parent().unwrap();

                let rel_to_masking = pathdiff::diff_paths(&abs_out_dir, abs_file_out_dir)
                    .unwrap_or_else(|| PathBuf::from("."));

                let mut path_str = utils::to_posix_path(&rel_to_masking.join("fragment-masking"));
                if !path_str.starts_with('.') && !path_str.starts_with('/') {
                    path_str.insert_str(0, "./");
                }
                path_str
            } else {
                "./fragment-masking".to_string()
            };

            let ctx = codegen::CodegenContext::new(
                &valid_schema,
                &params.project_context.fragment_to_path,
                &params.project_context.fragment_to_import,
                &params.project_context.fragment_to_type_only,
                &params.project_context.all_fragments,
                path,
                params.scalars,
                params.schema_import,
                params.type_imports,
                params.generate_ast_for_fragments,
                &params.project_context.fragment_dependencies,
                &shared_type_cache,
                params.document_suffix,
                params.variables_suffix,
                params.fragment_suffix,
                params.fragment_masking.clone(),
                masking_import_path,
            );

            execute_single_file_codegen(
                doc,
                &ctx,
                params.output_dir,
                params.base_dir,
                include_prefix,
                verbose,
            )
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
                                        std::fs::canonicalize(meta.path.as_ref()).unwrap_or_else(|_| PathBuf::from(meta.path.as_ref()))
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

    if success && let Some(out_dir) = params.output_dir {
        let out_dir_path = params.base_dir.join(out_dir);
        std::fs::create_dir_all(&out_dir_path).ok();
        if params.fragment_masking.is_enabled() {
            let masking_path = out_dir_path.join("fragment-masking.ts");
            let masking_content = codegen::generate_fragment_masking_file(
                params.fragment_masking.unmask_function_name(),
            );
            if let Err(e) = std::fs::write(&masking_path, masking_content) {
                eprintln!("{}: {}", "Failed to write fragment-masking".red(), e);
                success = false;
            }
        }
    }

    if success { Ok(all_ops) } else { Err(()) }
}

async fn clean_project_files(
    params: CodegenParams<'_>,
    verbose: bool,
) -> Result<Vec<codegen::OperationGenerated>, ()> {
    let glob_pattern = params.include.as_key();
    let include_prefix = utils::get_glob_root(&glob_pattern);
    let success = params
        .project_files
        .par_iter()
        .map(|path| {
            let out_path = utils::get_output_path(
                path,
                params.base_dir,
                params.output_dir,
                Some(include_prefix.to_str().unwrap_or("")),
            );
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

        if params.emit_permission_data {
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
    include_prefix: &str,
    verbose: bool,
) -> Result<Vec<codegen::OperationGenerated>, String> {
    let (ts_code, mut ops) = codegen::generate_typescript(doc, ctx)?;
    let out_path_raw = utils::get_output_path(
        doc.uri.to_file_path().unwrap().as_path(),
        base_dir,
        output_dir,
        Some(include_prefix),
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
