use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use colored::*;
use graphox_codegen as codegen;
use graphox_core::DocumentState;
use graphox_core::apollo_ast::AstEmitConfig;
use graphox_core::config::{Config, EmitExtensions, GlobPattern, NamingConvention, SchemaSource};
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
    pub output_dir: Option<&'a Path>,
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
    pub fragment_document_suffix: &'a str,
    pub query_suffix: &'a str,
    pub mutation_suffix: &'a str,
    pub subscription_suffix: &'a str,
    pub naming_convention: NamingConvention,
    pub fragment_masking: codegen::FragmentMasking,
    pub emit_extensions: graphox_core::config::EmitExtensions,
    pub ast_emit_config: graphox_core::apollo_ast::AstEmitConfig,
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
                let out_dir = config.base_dir.join(out);
                if let Ok(canon) = out_dir.canonicalize() {
                    output_dirs.push(canon);
                } else {
                    output_dirs.push(out_dir);
                }
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
                        if output_dirs
                            .iter()
                            .any(|d| utils::path_starts_with(&e.path, d))
                        {
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
    let mut project_fragments: HashMap<usize, Vec<codegen::FragmentGenerated>> = HashMap::new();

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
            continue;
        }

        let project_files: Vec<PathBuf> = project_meta
            .files
            .iter()
            .map(|p| cfg.base_dir.join(p))
            .collect();

        if project_files.is_empty() {
            continue;
        }

        let project_schema_files: HashSet<_> = project.schema.files().into_iter().collect();

        let project_output_dir = project.output_dir.as_deref().map(Path::new);

        let mut type_imports = HashMap::default();
        let mut schema_import = project.import.clone();

        if let Some(schema_types) = &cfg.schema_types {
            let mut matches: Vec<_> = schema_types
                .iter()
                .filter(|st| {
                    let st_files = st.schema.files();
                    st_files.iter().all(|f| project_schema_files.contains(f))
                })
                .collect();

            matches.sort_by_key(|st| std::cmp::Reverse(st.schema.files().len()));

            let project_abs_out_dir = project_output_dir.map(|d| cfg.base_dir.join(d));

            // 2. Build the type_imports map
            for st in matches.iter().rev() {
                if let Some(import_path) = &st.import
                    && let Ok(st_schema) =
                        schema::load_and_validate_schema(&cfg.base_dir, &st.schema)
                {
                    let mut final_import_path = import_path.clone();

                    if (final_import_path == "." || final_import_path == "./")
                        && let Some(abs_out_dir) = &project_abs_out_dir
                    {
                        let abs_st_output = cfg.base_dir.join(&st.output);
                        if abs_st_output.parent() == Some(abs_out_dir) {
                            let rel = pathdiff::diff_paths(&abs_st_output, abs_out_dir)
                                .unwrap_or_else(|| {
                                    PathBuf::from(abs_st_output.file_name().unwrap())
                                });
                            let mut s = utils::to_posix_path(&rel);
                            if s.ends_with(".ts") {
                                s.truncate(s.len() - 3);
                            }
                            if !s.starts_with('.') {
                                s = format!("./{}", s);
                            }
                            final_import_path = s;
                        }
                    }

                    for type_name in st_schema.types.keys() {
                        type_imports.insert(type_name.to_string(), final_import_path.clone());
                    }
                }
            }

            // 3. Keep schema_import for backward compatibility (the "best" match)
            if schema_import.is_none()
                && let Some(st) = matches.first()
            {
                let mut final_import_path = st.import.clone();
                if let Some(import_path) = &final_import_path
                    && (import_path == "." || import_path == "./")
                    && let Some(abs_out_dir) = &project_abs_out_dir
                {
                    let abs_st_output = cfg.base_dir.join(&st.output);
                    if abs_st_output.parent() == Some(abs_out_dir) {
                        let rel = pathdiff::diff_paths(&abs_st_output, abs_out_dir)
                            .unwrap_or_else(|| PathBuf::from(abs_st_output.file_name().unwrap()));
                        let mut s = utils::to_posix_path(&rel);
                        if s.ends_with(".ts") {
                            s.truncate(s.len() - 3);
                        }
                        if !s.starts_with('.') {
                            s = format!("./{}", s);
                        }
                        final_import_path = Some(s);
                    }
                }
                schema_import = final_import_path;
            }
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
            Engine::resolve_project_context(&valid_schema, global_metadata, &project_files);

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

        let pt_output = project.possible_types.as_ref();
        let tp_output = project.type_policies.as_ref();

        let pt_path = pt_output.map(|p| cfg.base_dir.join(p));
        let tp_path = tp_output.map(|p| cfg.base_dir.join(p));

        match (&pt_path, &tp_path) {
            (Some(pt), Some(tp)) if pt == tp => {
                if verbose {
                    println!(
                        "{}: {}",
                        "Generating possibleTypes and typePolicies".bright_black(),
                        pt.display().to_string().bright_black()
                    );
                }
                let pt_content = codegen::generate_possible_types(&valid_schema);
                let tp_content = codegen::generate_type_policies(&valid_schema);
                let combined =
                    format!("{}\n\n{}\n", pt_content.trim_end(), tp_content.trim_start());
                if let Err(e) = std::fs::write(pt, combined) {
                    eprintln!("{}: {}", "Failed to write combined output".red(), e);
                    success = false;
                }
            }
            _ => {
                if let Some(pt) = &pt_path {
                    if verbose {
                        println!(
                            "{}: {}",
                            "Generating possibleTypes".bright_black(),
                            pt.display().to_string().bright_black()
                        );
                    }
                    let content = codegen::generate_possible_types(&valid_schema);
                    if let Err(e) = std::fs::write(pt, content) {
                        eprintln!("{}: {}", "Failed to write possibleTypes".red(), e);
                        success = false;
                    }
                }

                if let Some(tp) = &tp_path {
                    if verbose {
                        println!(
                            "{}: {}",
                            "Generating typePolicies".bright_black(),
                            tp.display().to_string().bright_black()
                        );
                    }
                    let content = codegen::generate_type_policies(&valid_schema);
                    if let Err(e) = std::fs::write(tp, content) {
                        eprintln!("{}: {}", "Failed to write typePolicies".red(), e);
                        success = false;
                    }
                }
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
        let fragment_document_suffix = project
            .fragment_document_suffix
            .as_deref()
            .or(cfg.fragment_document_suffix.as_deref())
            .unwrap_or(document_suffix);
        let query_suffix = project
            .query_suffix
            .as_deref()
            .or(cfg.query_suffix.as_deref())
            .unwrap_or("Query");
        let mutation_suffix = project
            .mutation_suffix
            .as_deref()
            .or(cfg.mutation_suffix.as_deref())
            .unwrap_or("Mutation");
        let subscription_suffix = project
            .subscription_suffix
            .as_deref()
            .or(cfg.subscription_suffix.as_deref())
            .unwrap_or("Subscription");
        let naming_convention = project
            .naming_convention
            .clone()
            .or_else(|| cfg.naming_convention.clone())
            .unwrap_or_default();

        let emit_extensions = cfg.get_emit_extensions(project);
        match execute_project_codegen_entry(
            CodegenParams {
                base_dir: &cfg.base_dir,
                source: &project.schema,
                include: &project.include,
                project_files: &project_files,
                output_dir: project_output_dir,
                scalars: &cfg.scalars,
                schema_import: &schema_import,
                type_imports: &type_imports,
                project_context: &project_context,
                global_metadata,
                generate_ast_for_fragments: project
                    .generate_ast_for_fragments
                    .or(cfg.generate_ast_for_fragments)
                    .unwrap_or(false),
                workspace_documents: &workspace_metadata.documents,
                emit_permission_data: project.emit_permission_data.unwrap_or(false),
                document_suffix,
                variables_suffix,
                fragment_suffix,
                fragment_document_suffix,
                query_suffix,
                mutation_suffix,
                subscription_suffix,
                naming_convention,
                fragment_masking: codegen::FragmentMasking::from_config(
                    &project
                        .fragment_masking
                        .clone()
                        .or(cfg.fragment_masking.clone()),
                ),
                emit_extensions,
                ast_emit_config: AstEmitConfig::from_config(
                    project.emit_ast_directives.or(cfg.emit_ast_directives),
                    project.emit_ast_aliases.or(cfg.emit_ast_aliases),
                    project.emit_ast_arguments.or(cfg.emit_ast_arguments),
                    project
                        .emit_ast_variable_defaults
                        .or(cfg.emit_ast_variable_defaults),
                    project.inline_fragments.or(cfg.inline_fragments),
                ),
            },
            verbose,
            clean,
        )
        .await
        {
            Ok((ops, frags)) => {
                project_operations.insert(project_index, ops);
                project_fragments.insert(project_index, frags);
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

                if let Ok(schema) = schema::load_and_validate_schema(&cfg.base_dir, &st.schema) {
                    let pt_output = st.possible_types.as_ref();
                    let tp_output = st.type_policies.as_ref();

                    let pt_path = pt_output.map(|p| cfg.base_dir.join(p));
                    let tp_path = tp_output.map(|p| cfg.base_dir.join(p));

                    match (&pt_path, &tp_path) {
                        (Some(pt), Some(tp)) if pt == tp => {
                            if verbose {
                                println!(
                                    "{}: {}",
                                    "Generating possibleTypes and typePolicies".bright_black(),
                                    pt.display().to_string().bright_black()
                                );
                            }
                            let pt_content = codegen::generate_possible_types(&schema);
                            let tp_content = codegen::generate_type_policies(&schema);
                            let combined = format!(
                                "{}\n\n{}\n",
                                pt_content.trim_end(),
                                tp_content.trim_start()
                            );
                            if let Err(e) = std::fs::write(pt, combined) {
                                eprintln!("{}: {}", "Failed to write combined output".red(), e);
                                success = false;
                            }
                        }
                        _ => {
                            if let Some(pt) = &pt_path {
                                if verbose {
                                    println!(
                                        "{}: {}",
                                        "Generating possibleTypes".bright_black(),
                                        pt.display().to_string().bright_black()
                                    );
                                }
                                let content = codegen::generate_possible_types(&schema);
                                if let Err(e) = std::fs::write(pt, content) {
                                    eprintln!("{}: {}", "Failed to write possibleTypes".red(), e);
                                    success = false;
                                }
                            }

                            if let Some(tp) = &tp_path {
                                if verbose {
                                    println!(
                                        "{}: {}",
                                        "Generating typePolicies".bright_black(),
                                        tp.display().to_string().bright_black()
                                    );
                                }
                                let content = codegen::generate_type_policies(&schema);
                                if let Err(e) = std::fs::write(tp, content) {
                                    eprintln!("{}: {}", "Failed to write typePolicies".red(), e);
                                    success = false;
                                }
                            }
                        }
                    }
                } else {
                    eprintln!(
                        "{}: Failed to load schema for codegen generation",
                        "Error".red()
                    );
                    success = false;
                }
            }
        }
    }

    // Generate graphql.ts and manifest.json for each output directory
    if !clean {
        use std::collections::{BTreeMap, HashSet};
        let mut dir_to_ops: BTreeMap<PathBuf, Vec<codegen::OperationGenerated>> = BTreeMap::new();
        let mut dir_to_frags: BTreeMap<PathBuf, Vec<codegen::FragmentGenerated>> = BTreeMap::new();
        let mut dir_to_config: BTreeMap<
            PathBuf,
            (
                codegen::FragmentMasking,
                String,
                String,
                EmitExtensions,
                bool,
                bool,
            ),
        > = BTreeMap::new();

        let mut project_indices: HashSet<usize> = project_operations.keys().cloned().collect();
        project_indices.extend(project_fragments.keys().cloned());

        for project_idx in project_indices {
            let Some(project) = cfg.projects.get(project_idx) else {
                continue;
            };

            let ops = project_operations
                .get(&project_idx)
                .cloned()
                .unwrap_or_default();
            let frags = project_fragments
                .get(&project_idx)
                .cloned()
                .unwrap_or_default();

            let out_dir = project.output_dir.as_deref().unwrap_or("__generated__");
            let out_dir_path = cfg.base_dir.join(out_dir);
            let canon_out_dir_path = out_dir_path
                .canonicalize()
                .unwrap_or_else(|_| out_dir_path.clone());

            dir_to_ops
                .entry(canon_out_dir_path.clone())
                .or_default()
                .extend(ops);

            dir_to_frags
                .entry(canon_out_dir_path.clone())
                .or_default()
                .extend(frags);

            if let std::collections::btree_map::Entry::Vacant(e) =
                dir_to_config.entry(canon_out_dir_path)
            {
                let fragment_masking = codegen::FragmentMasking::from_config(
                    &project
                        .fragment_masking
                        .clone()
                        .or(cfg.fragment_masking.clone()),
                );
                let document_suffix = project
                    .document_suffix
                    .as_deref()
                    .or(cfg.document_suffix.as_deref())
                    .unwrap_or("Document")
                    .to_string();
                let variables_suffix = project
                    .variables_suffix
                    .as_deref()
                    .or(cfg.variables_suffix.as_deref())
                    .unwrap_or("Variables")
                    .to_string();
                e.insert((
                    fragment_masking,
                    document_suffix,
                    variables_suffix,
                    cfg.get_emit_extensions(project),
                    project
                        .generate_ast_for_fragments
                        .or(cfg.generate_ast_for_fragments)
                        .unwrap_or(false),
                    project.re_exports.or(cfg.re_exports).unwrap_or(false),
                ));
            }
        }

        for (out_dir_path, mut ops) in dir_to_ops.into_iter() {
            let mut frags = dir_to_frags.remove(&out_dir_path).unwrap_or_default();
            let (
                fragment_masking,
                doc_suffix,
                var_suffix,
                emit_extensions,
                generate_ast,
                re_exports,
            ) = dir_to_config.get(&out_dir_path).unwrap();

            // Deduplicate operations by name and source
            ops.sort_by(|a, b| {
                a.operation_type_name
                    .cmp(&b.operation_type_name)
                    .then_with(|| a.source_text.cmp(&b.source_text))
            });
            ops.dedup_by(|a, b| {
                a.operation_type_name == b.operation_type_name && a.source_text == b.source_text
            });

            // Deduplicate fragments by source
            frags.sort_by(|a, b| a.source_text.cmp(&b.source_text));
            frags.dedup_by(|a, b| a.source_text == b.source_text);

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

            if verbose {
                println!(
                    "{}: {}",
                    "Generating entrypoint".bright_black(),
                    entrypoint_path.display().to_string().bright_black()
                );
            }
            let content = codegen::generate_entrypoint_content(
                &out_dir_path,
                &ops,
                &frags,
                doc_suffix,
                var_suffix,
                fragment_masking,
                *emit_extensions,
                *generate_ast,
                *re_exports,
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
                        "name": format!("{}{}", op.operation_type_name, doc_suffix)
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
) -> Result<
    (
        Vec<codegen::OperationGenerated>,
        Vec<codegen::FragmentGenerated>,
    ),
    (),
> {
    if !clean {
        generate_project_files(params, verbose).await
    } else {
        clean_project_files(params, verbose).await
    }
}

async fn generate_project_files(
    params: CodegenParams<'_>,
    verbose: bool,
) -> Result<
    (
        Vec<codegen::OperationGenerated>,
        Vec<codegen::FragmentGenerated>,
    ),
    (),
> {
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
        .filter_map(|path| {
            params.workspace_documents.get(path).map(|doc| (path, doc))
        })
        .filter(|(_, doc)| {
            !doc.get_graphql_trees().is_empty()
        })
        .map(|(path, doc)| {
            let patterns = params.include.patterns();
            let include_prefix_path = patterns
                .iter()
                .map(|p| {
                    utils::get_glob_root(p)
                })
                .find(|root| {
                    let abs_root = params.base_dir.join(root);
                    path.starts_with(&abs_root)
                })
                .unwrap_or_default();

            let out_path_raw = utils::get_output_path(
                path,
                params.base_dir,
                params.output_dir,
                Some(&include_prefix_path),
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
                path_str.push_str(params.emit_extensions.as_str());
                path_str
            } else {
                let mut path_str = "./fragment-masking".to_string();
                path_str.push_str(params.emit_extensions.as_str());
                path_str
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
                params.fragment_document_suffix,
                params.query_suffix,
                params.mutation_suffix,
                params.subscription_suffix,
                params.naming_convention.clone(),
                params.fragment_masking.clone(),
                masking_import_path,
                params.emit_extensions,
                params.ast_emit_config.clone(),
                abs_out_path.clone(),
            );

            execute_single_file_codegen(
                doc,
                &ctx,
                params.output_dir,
                params.base_dir,
                &include_prefix_path,
                verbose,
            )
            .map_err(|e| (path.to_path_buf(), e))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|res| match res {
            Ok((ops, frags)) => Ok((ops, frags)),
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
                    Ok((Vec::new(), Vec::new()))
                }
            }
        })
        .collect();

    let mut all_ops = Vec::new();
    let mut all_frags = Vec::new();
    let mut success = true;
    for res in results {
        match res {
            Ok((ops, frags)) => {
                all_ops.extend(ops);
                all_frags.extend(frags);
            }
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

        let index_path = out_dir_path.join("index.ts");
        let index_content =
            codegen::generate_index_content(&params.fragment_masking, params.emit_extensions);
        if let Err(e) = std::fs::write(&index_path, index_content) {
            eprintln!("{}: {}", "Failed to write index.ts".red(), e);
            success = false;
        }
    }

    if success {
        Ok((all_ops, all_frags))
    } else {
        Err(())
    }
}

async fn clean_project_files(
    params: CodegenParams<'_>,
    verbose: bool,
) -> Result<
    (
        Vec<codegen::OperationGenerated>,
        Vec<codegen::FragmentGenerated>,
    ),
    (),
> {
    let include_root = utils::get_glob_root(&params.include.as_key());
    let abs_include_root = params.base_dir.join(&include_root);

    match params.output_dir {
        Some(out_dir) => {
            let abs_out_dir = params.base_dir.join(out_dir);

            if abs_out_dir != abs_include_root {
                if abs_out_dir.exists() {
                    if let Err(e) = std::fs::remove_dir_all(&abs_out_dir) {
                        eprintln!(
                            "{}: {} - {}",
                            "Failed to remove output directory".red(),
                            abs_out_dir.display().to_string().red(),
                            e
                        );
                        return Err(());
                    } else if verbose {
                        println!(
                            "{}: {}",
                            "Removed directory".bright_black(),
                            abs_out_dir.display().to_string().bright_black()
                        );
                    }
                }
            } else {
                eprintln!(
                    "{}: output_dir '{}' is the same as include root, performing surgical cleanup",
                    "Warning".yellow(),
                    out_dir.display()
                );
                surgical_clean(&abs_out_dir, verbose)?;
            }
        }
        None => {
            params
                .project_files
                .par_iter()
                .map(|path| {
                    let out_path = utils::get_output_path(
                        path,
                        params.base_dir,
                        params.output_dir,
                        Some(&include_root),
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
        }
    }

    Ok((Vec::new(), Vec::new()))
}

fn surgical_clean(dir: &Path, verbose: bool) -> Result<(), ()> {
    let mut ok = true;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "codegen.ts").unwrap_or(false) {
                if let Err(e) = std::fs::remove_file(&path) {
                    eprintln!(
                        "{}: {} - {}",
                        "Failed to remove".red(),
                        path.display().to_string().red(),
                        e
                    );
                    ok = false;
                } else if verbose {
                    println!(
                        "{}: {}",
                        "Removed".bright_black(),
                        path.display().to_string().bright_black()
                    );
                }
            }
        }
    }

    let known_files = [
        "graphql.ts",
        "manifest.json",
        "permissions.ts",
        "fragment-masking.ts",
        "index.ts",
    ];
    for name in &known_files {
        let path = dir.join(name);
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                eprintln!(
                    "{}: {} - {}",
                    "Failed to remove".red(),
                    path.display().to_string().red(),
                    e
                );
                ok = false;
            } else if verbose {
                println!(
                    "{}: {}",
                    "Removed".bright_black(),
                    path.display().to_string().bright_black()
                );
            }
        }
    }

    if ok { Ok(()) } else { Err(()) }
}

fn execute_single_file_codegen(
    doc: &DocumentState,
    ctx: &codegen::CodegenContext<'_>,
    output_dir: Option<&Path>,
    base_dir: &Path,
    include_prefix: &Path,
    verbose: bool,
) -> Result<
    (
        Vec<codegen::OperationGenerated>,
        Vec<codegen::FragmentGenerated>,
    ),
    String,
> {
    let (ts_code, mut ops, mut frags) = codegen::generate_typescript(doc, ctx)?;
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

    for frag in &mut frags {
        frag.codegen_path = abs_out_path.clone();
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
    Ok((ops, frags))
}
