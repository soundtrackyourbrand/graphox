use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use colored::*;
use graphox_codegen as codegen;
use graphox_core::DocumentState;
use graphox_core::config::{CodegenConfig, Config, GlobPattern, SchemaSource};
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
    pub scalars: &'a HashMap<String, String>,
    pub schema_import: &'a Option<String>,
    pub type_imports: &'a HashMap<String, String>,
    pub project_context: &'a ProjectContext,
    pub global_metadata: &'a [FragmentMetadata],
    pub generate_ast_for_fragments: bool,
    pub workspace_documents: &'a HashMap<PathBuf, DocumentState>,
    pub codegen_config: &'a CodegenConfig,
    pub emit_extensions: graphox_core::config::EmitExtensions,
    pub use_cache: bool,
    pub type_cache: &'a codegen::SchemaAnalysisCaches,
}

pub async fn run_codegen(mut config: Config, watch: bool, verbose: bool, clean: bool) {
    if !watch {
        if !execute_codegen(config, verbose, clean).await {
            eprintln!("{}", "Codegen failed.".red());
            graphox_core::utils::flush_stdio();
            std::process::exit(1);
        }
        return;
    }

    'watch_loop: loop {
        println!("{}", "Watching for changes...".bright_black());
        let _ = execute_codegen(config.clone(), verbose, false).await;

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let (config_tx, mut config_rx) = tokio::sync::mpsc::channel(1);

        let gitignore = utils::get_gitignore_matcher(config.base_dir());
        let config_tx_clone = config_tx.clone();
        let base_dir_for_watcher = config.base_dir().to_path_buf();
        let config_for_watcher = config.clone();
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
                        if config_for_watcher.is_output_file(&e.path) {
                            return false;
                        }
                        if !should_trigger_codegen_for_path(&e.path) {
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

        for project in config.projects() {
            for pattern in project.include().patterns() {
                let watch_path = config.base_dir().join(utils::get_glob_root(&pattern));
                debouncer
                    .watcher()
                    .watch(&watch_path, notify::RecursiveMode::Recursive)
                    .ok();
            }
        }

        for project in config.projects() {
            for file in project.schema().files() {
                debouncer
                    .watcher()
                    .watch(
                        &config.base_dir().join(file),
                        notify::RecursiveMode::NonRecursive,
                    )
                    .ok();
            }
        }
        let schema_types = config.schema_types();
        if !schema_types.is_empty() {
            for st in schema_types {
                for file in st.schema().files() {
                    debouncer
                        .watcher()
                        .watch(
                            &config.base_dir().join(file),
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

                    if let Ok(Some(new_config)) = Config::load_from_dir(config.base_dir()) {
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

fn should_trigger_codegen_for_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let is_host_file = matches!(
        ext,
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
    );
    if !is_host_file {
        return true;
    }

    let Ok(content) = std::fs::read_to_string(path) else {
        return true;
    };
    let bytes = content.as_bytes();
    bytes.windows(3).any(|w| w.eq_ignore_ascii_case(b"gql"))
        || bytes.windows(7).any(|w| w.eq_ignore_ascii_case(b"graphql"))
}

async fn execute_codegen(config: Config, verbose: bool, clean: bool) -> bool {
    let mut success = true;

    let cfg = config;

    if clean {
        if let Err(e) = schema_cache::clear_cache() {
            eprintln!("{}: {}", "Failed to clear schema cache".red(), e);
            success = false;
        } else if verbose {
            println!("{}", "Cleared schema cache".bright_black());
        }
    }

    if clean {
        return execute_clean_only(&cfg, verbose) && success;
    }

    let workspace_metadata =
        Engine::scan_workspace(&cfg, tower_lsp::lsp_types::PositionEncodingKind::UTF8, None);
    let global_metadata = &workspace_metadata.fragments;

    let shared_caches = codegen::SchemaAnalysisCaches::new();

    // Pre-calculate type imports for all projects to avoid redundant work in the project loop
    let mut workspace_type_imports: HashMap<String, HashMap<String, String>> = HashMap::new();
    let schema_types = cfg.schema_types();

    // Collect unique schema sources to process in parallel
    let mut unique_sources = HashSet::new();
    for project in cfg.projects() {
        unique_sources.insert(project.schema().as_key());
    }

    let source_to_matches: HashMap<String, Vec<_>> = unique_sources
        .iter()
        .map(|key| {
            let schema_files: HashSet<_> = key.split(',').map(String::from).collect();
            let mut matches: Vec<_> = schema_types
                .iter()
                .filter(|st| {
                    let st_files = st.schema().files();
                    st_files.iter().all(|f| schema_files.contains(f))
                })
                .collect();
            matches.sort_by_key(|st| std::cmp::Reverse(st.schema().files().len()));
            (key.clone(), matches)
        })
        .collect();

    let pre_calculated_imports: std::collections::HashMap<String, HashMap<String, String>> =
        unique_sources
            .par_iter()
            .map(|key| {
                let mut project_type_imports = HashMap::default();
                if let Some(matches) = source_to_matches.get(key) {
                    for st in matches.iter().rev() {
                        if let Some(import_path) = st.import()
                            && let Ok(st_schema) = schema::load_schema_with_cache(
                                cfg.base_dir(),
                                st.schema(),
                                cfg.enable_schema_cache(),
                            )
                        {
                            for type_name in st_schema.types.keys() {
                                project_type_imports
                                    .insert(type_name.to_string(), import_path.to_string());
                            }
                        }
                    }
                }
                (key.clone(), project_type_imports)
            })
            .collect();
    for (k, v) in pre_calculated_imports {
        workspace_type_imports.insert(k, v);
    }

    // Process projects in parallel
    let project_results: Vec<_> = cfg
        .projects()
        .par_iter()
        .enumerate()
        .filter_map(|(project_index, project)| {
            if !cfg.get_project_codegen_enabled(project) && !clean {
                return None;
            }

            let project_meta = &workspace_metadata.projects[project_index];
            let project_files: Vec<PathBuf> = project_meta
                .files
                .iter()
                .map(|p| cfg.base_dir().join(p))
                .collect();

            if project_files.is_empty() {
                return None;
            }

            let project_output_dir = project.output_dir().map(Path::new);
            let type_imports = workspace_type_imports
                .get(&project.schema().as_key())
                .unwrap();
            let mut schema_import = project.import().map(String::from);

            if schema_import.is_none()
                && let Some(matches) = source_to_matches.get(&project.schema().as_key())
                && let Some(st) = matches.first()
            {
                let project_abs_out_dir = project_output_dir.map(|d| cfg.base_dir().join(d));
                let mut final_import_path = st.import().map(String::from);
                if let Some(import_path) = &final_import_path
                    && (import_path == "." || import_path == "./")
                    && let Some(abs_out_dir) = &project_abs_out_dir
                {
                    let abs_st_output = cfg.base_dir().join(st.output());
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

            let valid_schema = match schema::load_schema_with_cache(
                cfg.base_dir(),
                project.schema(),
                cfg.enable_schema_cache(),
            ) {
                Ok(v) => match v.validate() {
                    Ok(valid) => valid,
                    Err(e) => {
                        // Validation failed: report and treat as project failure (do not panic)
                        eprintln!("{}", e.to_string().red());
                        return Some(Err(()));
                    }
                },
                Err(e) => {
                    eprintln!("{}", e.to_string().red());
                    return Some(Err(()));
                }
            };

            let project_context = match Engine::resolve_project_context(
                &valid_schema,
                global_metadata,
                &project_files,
            ) {
                Ok(ctx) => ctx,
                Err(e) => {
                    eprintln!("{}: {}", "Error resolving project context".red(), e.red());
                    return Some(Err(()));
                }
            };

            let codegen_config = cfg.get_codegen_config(Some(project));
            let emit_extensions = cfg.get_emit_extensions(project);

            let res = execute_project_codegen_sync(
                CodegenParams {
                    base_dir: cfg.base_dir(),
                    source: project.schema(),
                    include: project.include(),
                    project_files: &project_files,
                    output_dir: project_output_dir,
                    scalars: cfg.scalars(),
                    schema_import: &schema_import,
                    type_imports,
                    project_context: &project_context,
                    global_metadata,
                    generate_ast_for_fragments: codegen_config.generate_ast_for_fragments(),
                    workspace_documents: &workspace_metadata.documents,
                    codegen_config: &codegen_config,
                    emit_extensions,
                    use_cache: cfg.enable_schema_cache(),
                    type_cache: &shared_caches,
                },
                verbose,
                clean,
            );

            match res {
                Ok((ops, frags)) => Some(Ok((project_index, ops, frags))),
                Err(_) => Some(Err(())),
            }
        })
        .collect();

    let mut project_operations: HashMap<usize, Vec<codegen::OperationGenerated>> = HashMap::new();
    let mut project_fragments: HashMap<usize, Vec<codegen::FragmentGenerated>> = HashMap::new();

    for res in project_results {
        match res {
            Ok((idx, ops, frags)) => {
                project_operations.insert(idx, ops);
                project_fragments.insert(idx, frags);
            }
            Err(_) => success = false,
        }
    }

    if !schema_types.is_empty() && (clean || cfg.codegen().is_enabled()) {
        let schema_results: Vec<_> = schema_types
            .par_iter()
            .map(|st| {
                let abs_output = cfg.base_dir().join(st.output());
                if clean {
                    if abs_output.exists() {
                        if let Err(e) = std::fs::remove_file(&abs_output) {
                            eprintln!(
                                "{}: {} - {}",
                                "Failed to remove".red(),
                                abs_output.display().to_string().red(),
                                e
                            );
                            return Err(());
                        } else if verbose {
                            println!(
                                "{}: {}",
                                "Removed".bright_black(),
                                abs_output.display().to_string().bright_black()
                            );
                        }
                    }
                } else {
                    if !clean && verbose {
                        println!("Generating types for schema: {}", st.output().blue());
                    }
                    let res = execute_schema_codegen_sync(
                        cfg.base_dir(),
                        st.schema(),
                        &abs_output.to_string_lossy(),
                        cfg.scalars(),
                        verbose,
                        cfg.enable_schema_cache(),
                    );

                    if !res {
                        return Err(());
                    }

                    if let Ok(schema) = schema::load_schema_with_cache(
                        cfg.base_dir(),
                        st.schema(),
                        cfg.enable_schema_cache(),
                    ) {
                        let valid_schema = schema.validate().expect("Schema should be valid");
                        let pt_output = st.possible_types();
                        let tp_output = st.type_policies();

                        let pt_path = pt_output.map(|p| cfg.base_dir().join(p));
                        let tp_path = tp_output.map(|p| cfg.base_dir().join(p));

                        match (&pt_path, &tp_path) {
                            (Some(pt), Some(tp)) if pt == tp => {
                                let pt_content = codegen::generate_possible_types(&valid_schema);
                                let tp_content = codegen::generate_type_policies(&valid_schema);
                                let combined = format!(
                                    "{}\n\n{}\n",
                                    pt_content.trim_end(),
                                    tp_content.trim_start()
                                );
                                let mut should_write = true;
                                if pt.exists()
                                    && let Ok(existing) = std::fs::read_to_string(pt)
                                    && existing == combined
                                {
                                    should_write = false;
                                }
                                if should_write && let Err(e) = std::fs::write(pt, combined) {
                                    eprintln!("{}: {}", "Failed to write combined output".red(), e);
                                    return Err(());
                                }
                            }
                            _ => {
                                if let Some(pt) = &pt_path {
                                    let content = codegen::generate_possible_types(&valid_schema);
                                    let mut should_write = true;
                                    if pt.exists()
                                        && let Ok(existing) = std::fs::read_to_string(pt)
                                        && existing == content
                                    {
                                        should_write = false;
                                    }
                                    if should_write && let Err(e) = std::fs::write(pt, content) {
                                        eprintln!(
                                            "{}: {}",
                                            "Failed to write possibleTypes".red(),
                                            e
                                        );
                                        return Err(());
                                    }
                                }

                                if let Some(tp) = &tp_path {
                                    let content = codegen::generate_type_policies(&valid_schema);
                                    let mut should_write = true;
                                    if tp.exists()
                                        && let Ok(existing) = std::fs::read_to_string(tp)
                                        && existing == content
                                    {
                                        should_write = false;
                                    }
                                    if should_write && let Err(e) = std::fs::write(tp, content) {
                                        eprintln!(
                                            "{}: {}",
                                            "Failed to write typePolicies".red(),
                                            e
                                        );
                                        return Err(());
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            })
            .collect();

        if schema_results.iter().any(|r| r.is_err()) {
            success = false;
        }
    }

    // Generate graphql.ts and manifest.json for each output directory
    if !clean {
        use std::collections::{BTreeMap, HashSet};
        let mut dir_to_ops: BTreeMap<PathBuf, Vec<codegen::OperationGenerated>> = BTreeMap::new();
        let mut dir_to_frags: BTreeMap<PathBuf, Vec<codegen::FragmentGenerated>> = BTreeMap::new();
        let mut dir_to_config: BTreeMap<PathBuf, CodegenConfig> = BTreeMap::new();
        let mut dir_to_schema_import: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();

        let mut project_indices: HashSet<usize> = project_operations.keys().cloned().collect();
        project_indices.extend(project_fragments.keys().cloned());

        for project_idx in project_indices {
            let Some(project) = cfg.projects().get(project_idx) else {
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

            let out_dir = project.output_dir().unwrap_or("__generated__");
            let out_dir_path = cfg.base_dir().join(out_dir);
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
                dir_to_config.entry(canon_out_dir_path.clone())
            {
                e.insert(cfg.get_codegen_config(Some(project)));
            }

            if let std::collections::btree_map::Entry::Vacant(e) =
                dir_to_schema_import.entry(canon_out_dir_path.clone())
            {
                let project_codegen = cfg.get_codegen_config(Some(project));
                let schema_import = project_codegen
                    .schema_import()
                    .map(String::from)
                    .or_else(|| project.import().map(String::from));
                e.insert(schema_import);
            }
        }

        let dir_data: Vec<_> = dir_to_ops
            .into_iter()
            .map(|(path, ops)| {
                let frags = dir_to_frags.remove(&path).unwrap_or_default();
                let config = dir_to_config.get(&path).unwrap().clone();
                let schema_import = dir_to_schema_import.get(&path).and_then(|o| o.clone());
                (path, ops, frags, config, schema_import)
            })
            .collect();

        let dir_results: Vec<_> = dir_data
            .into_par_iter()
            .map(
                |(out_dir_path, mut ops, mut frags, codegen_config, schema_import)| {
                    // Deduplicate operations by name and source
                    ops.sort_by(|a, b| {
                        a.operation_type_name
                            .cmp(&b.operation_type_name)
                            .then_with(|| a.source_text.cmp(&b.source_text))
                    });
                    ops.dedup_by(|a, b| {
                        a.operation_type_name == b.operation_type_name
                            && a.source_text == b.source_text
                    });

                    // Deduplicate fragments by name and source
                    frags.sort_by(|a, b| {
                        a.fragment_type_name
                            .cmp(&b.fragment_type_name)
                            .then_with(|| a.source_text.cmp(&b.source_text))
                    });
                    frags.dedup_by(|a, b| {
                        a.fragment_type_name == b.fragment_type_name
                            && a.source_text == b.source_text
                    });

                    // Check if path exists but is a file (blocks directory creation)
                    if out_dir_path.exists() && out_dir_path.is_file() {
                        eprintln!(
                            "{}: output_dir '{}' exists as a file, not a directory",
                            "Error".red(),
                            out_dir_path.display()
                        );
                        return Err(());
                    }

                    let entrypoint_path =
                        out_dir_path.join(format!("{}.ts", codegen_config.entrypoint_name()));

                    let content = codegen::generate_entrypoint_content(
                        &out_dir_path,
                        &ops,
                        &frags,
                        &codegen_config,
                        codegen_config.re_exports(),
                        schema_import.as_deref(),
                    );
                    if let Err(e) = std::fs::create_dir_all(&out_dir_path) {
                        eprintln!(
                            "{}: Failed to create directory '{}' - {}",
                            "Error".red(),
                            out_dir_path.display(),
                            e
                        );
                        return Err(());
                    }

                    // Write entrypoint only if changed
                    let mut write_entry = true;
                    if entrypoint_path.exists()
                        && let Ok(existing) = std::fs::read_to_string(&entrypoint_path)
                        && existing == content
                    {
                        write_entry = false;
                    }
                    if write_entry && let Err(e) = std::fs::write(&entrypoint_path, content) {
                        eprintln!(
                            "{}: {} (entrypoint: {}, dir exists: {})",
                            "Failed to write entrypoint".red(),
                            e,
                            entrypoint_path.display(),
                            out_dir_path.exists()
                        );
                        return Err(());
                    }

                    let manifest_path = out_dir_path.join("manifest.json");
                    let generate_ast_for_frags = codegen_config.generate_ast_for_fragments();
                    let manifest_entries: Vec<_> = ops
                        .iter()
                        .map(|op| {
                            let rel_path = pathdiff::diff_paths(&op.codegen_path, &out_dir_path)
                                .unwrap_or_else(|| op.codegen_path.clone());
                            let mut path_str = utils::to_posix_path(&rel_path);
                            if !path_str.starts_with('.')
                                && !path_str.starts_with('/')
                                && !rel_path.is_absolute()
                            {
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
                                "name": op.document_name
                            })
                        })
                        .chain(
                            frags
                                .iter()
                                .filter(|_| {
                                    generate_ast_for_frags
                                        || codegen_config.fragment_masking_mode().is_enabled()
                                })
                                .map(|frag| {
                                    let rel_path =
                                        pathdiff::diff_paths(&frag.codegen_path, &out_dir_path)
                                            .unwrap_or_else(|| frag.codegen_path.clone());
                                    let mut path_str = utils::to_posix_path(&rel_path);
                                    if !path_str.starts_with('.')
                                        && !path_str.starts_with('/')
                                        && !rel_path.is_absolute()
                                    {
                                        path_str = format!("./{}", path_str);
                                    }
                                    let path_no_ext = if path_str.ends_with(".ts") {
                                        &path_str[..path_str.len() - 3]
                                    } else {
                                        &path_str
                                    };

                                    sonic_rs::json!({
                                        "source": frag.source_text,
                                        "path": path_no_ext,
                                        "name": frag.document_name
                                    })
                                }),
                        )
                        .collect();

                    let manifest_json = sonic_rs::to_string_pretty(&manifest_entries).unwrap();

                    let mut write_manifest = true;
                    if manifest_path.exists()
                        && let Ok(existing) = std::fs::read_to_string(&manifest_path)
                        && existing == manifest_json
                    {
                        write_manifest = false;
                    }
                    if write_manifest && let Err(e) = std::fs::write(&manifest_path, manifest_json)
                    {
                        eprintln!(
                            "{}: {} (manifest: {}, dir exists: {})",
                            "Failed to write manifest".red(),
                            e,
                            manifest_path.display(),
                            out_dir_path.exists()
                        );
                        return Err(());
                    }
                    Ok(())
                },
            )
            .collect();

        if dir_results.iter().any(|r| r.is_err()) {
            success = false;
        }
    }

    success
}

fn execute_clean_only(cfg: &Config, verbose: bool) -> bool {
    let mut success = true;

    for project in cfg.projects() {
        let output_dir = project.output_dir().map(Path::new);
        let project_files = if output_dir.is_none() {
            utils::get_project_scan_files(cfg, project, None)
                .into_iter()
                .map(|path| {
                    if path.is_absolute() {
                        path
                    } else {
                        cfg.base_dir().join(path)
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let res = clean_project_files_sync(
            CleanParams {
                base_dir: cfg.base_dir(),
                include: project.include(),
                project_files: &project_files,
                output_dir,
                entrypoint_name: project.codegen().entrypoint_name(),
            },
            verbose,
        );

        if res.is_err() {
            success = false;
        }
    }

    let schema_types = cfg.schema_types();
    if !schema_types.is_empty() {
        let schema_results: Vec<_> = schema_types
            .par_iter()
            .map(|st| {
                let abs_output = cfg.base_dir().join(st.output());
                if abs_output.exists() {
                    if let Err(e) = std::fs::remove_file(&abs_output) {
                        eprintln!(
                            "{}: {} - {}",
                            "Failed to remove".red(),
                            abs_output.display().to_string().red(),
                            e
                        );
                        return Err(());
                    } else if verbose {
                        println!(
                            "{}: {}",
                            "Removed".bright_black(),
                            abs_output.display().to_string().bright_black()
                        );
                    }
                }
                Ok(())
            })
            .collect();

        if schema_results.iter().any(|r| r.is_err()) {
            success = false;
        }
    }

    success
}

fn execute_schema_codegen_sync(
    base_dir: &Path,
    source: &SchemaSource,
    output_path: &str,
    scalars: &HashMap<String, String>,
    verbose: bool,
    use_cache: bool,
) -> bool {
    let valid_schema = match schema::load_schema_with_cache(base_dir, source, use_cache) {
        Ok(v) => v.validate().expect("Schema should be valid"),
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

    let mut should_write = true;
    if out_path.exists()
        && let Ok(existing) = std::fs::read_to_string(out_path)
        && existing == ts_code
    {
        should_write = false;
    }

    if should_write {
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
    }
    true
}

fn execute_project_codegen_sync(
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
        generate_project_files_sync(params, verbose)
    } else {
        clean_project_files_sync(
            CleanParams {
                base_dir: params.base_dir,
                include: params.include,
                project_files: params.project_files,
                output_dir: params.output_dir,
                entrypoint_name: params.codegen_config.entrypoint_name(),
            },
            verbose,
        )
    }
}

struct CleanParams<'a> {
    base_dir: &'a Path,
    include: &'a graphox_core::config::GlobPattern,
    project_files: &'a [PathBuf],
    output_dir: Option<&'a Path>,
    entrypoint_name: &'a str,
}

fn generate_project_files_sync(
    params: CodegenParams<'_>,
    verbose: bool,
) -> Result<
    (
        Vec<codegen::OperationGenerated>,
        Vec<codegen::FragmentGenerated>,
    ),
    (),
> {
    let valid_schema =
        match schema::load_schema_with_cache(params.base_dir, params.source, params.use_cache) {
            Ok(v) => v.validate().expect("Schema should be valid"),
            Err(e) => {
                eprintln!("{}", e.to_string().red());
                return Err(());
            }
        };

    let results: Vec<_> = params
        .project_files
        .par_iter()
        .filter_map(|path| params.workspace_documents.get(path).map(|doc| (path, doc)))
        .filter(|(_, doc)| !doc.get_graphql_trees().is_empty())
        .map(|(path, doc)| {
            let patterns = params.include.patterns();
            let include_prefix_path = patterns
                .iter()
                .map(|p| utils::get_glob_root(p))
                .find(|root| {
                    let abs_root = params.base_dir.join(root);
                    let abs_root = std::fs::canonicalize(&abs_root).unwrap_or(abs_root);
                    utils::path_starts_with(path, &abs_root)
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

                let full_masking_path = rel_to_masking.join("fragment-masking");
                let mut path_str = utils::to_posix_path(&full_masking_path);
                if !path_str.starts_with('.') && !path_str.starts_with('/') && !full_masking_path.is_absolute() {
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
                &params.project_context.name_to_id,
                path,
                params.scalars,
                params.schema_import,
                params.type_imports,
                params.codegen_config.generate_ast_for_fragments(),
                &params.project_context.fragment_dependencies,
                params.type_cache,
                params.codegen_config,
                masking_import_path,
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
        let fragment_masking =
            codegen::FragmentMasking::from_core_config(&params.codegen_config.fragment_masking());
        if fragment_masking.is_enabled() {
            let masking_path = out_dir_path.join("fragment-masking.ts");
            let masking_content =
                codegen::generate_fragment_masking_file(fragment_masking.unmask_function_name());
            if let Err(e) = std::fs::write(&masking_path, masking_content) {
                eprintln!("{}: {}", "Failed to write fragment-masking".red(), e);
                success = false;
            }
        }

        let index_path = out_dir_path.join("index.ts");
        let index_content = codegen::generate_index_content(
            &fragment_masking,
            params.emit_extensions,
            params.codegen_config.entrypoint_name(),
        );
        if let Err(e) = std::fs::write(&index_path, index_content) {
            eprintln!("{}: {}", "Failed to write index.ts".red(), e);
            success = false;
        }

        // Optionally emit permission metadata for the project
        if params.codegen_config.emit_permission_data() {
            if let Some(_out_dir) = params.output_dir {
                let permissions_path = out_dir_path.join("permissions.ts");
                let content = codegen::emit_permission_data_content(
                    &valid_schema,
                    params.scalars,
                    params.schema_import,
                );
                if let Err(e) = std::fs::write(&permissions_path, content) {
                    eprintln!("{}: {}", "Failed to write permissions.ts".red(), e);
                    success = false;
                }
            } else {
                eprintln!(
                    "{}: emit_permission_data is enabled but no output_dir is specified for project.",
                    "Warning".yellow()
                );
                // Do not fail the whole run for missing output_dir, just warn
            }
        }
    }

    if success {
        Ok((all_ops, all_frags))
    } else {
        Err(())
    }
}

fn clean_project_files_sync(
    params: CleanParams<'_>,
    verbose: bool,
) -> Result<
    (
        Vec<codegen::OperationGenerated>,
        Vec<codegen::FragmentGenerated>,
    ),
    (),
> {
    let patterns = params.include.patterns();

    match params.output_dir {
        Some(out_dir) => {
            let abs_out_dir = params.base_dir.join(out_dir);
            let is_surgical = utils::output_dir_requires_surgical_handling(
                params.base_dir,
                &patterns,
                &abs_out_dir,
            );

            if !is_surgical {
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
                    "{}: output_dir '{}' is the same as an include root, performing surgical cleanup",
                    "Warning".yellow(),
                    out_dir.display()
                );
                surgical_clean(&abs_out_dir, verbose, params.entrypoint_name)?;
            }
        }
        None => {
            // Clean individual files
            params
                .project_files
                .par_iter()
                .map(|path| {
                    let include_prefix_path = patterns
                        .iter()
                        .map(|p| utils::get_glob_root(p))
                        .find(|root| {
                            let abs_root = params.base_dir.join(root);
                            let abs_root = std::fs::canonicalize(&abs_root).unwrap_or(abs_root);
                            utils::path_starts_with(path, &abs_root)
                        })
                        .unwrap_or_default();

                    let out_path_raw = utils::get_output_path(
                        path,
                        params.base_dir,
                        params.output_dir,
                        Some(&include_prefix_path),
                    );

                    let out_path = if out_path_raw.is_absolute() {
                        out_path_raw
                    } else {
                        params.base_dir.join(out_path_raw)
                    };

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

            // Also clean up default __generated__ directory if it exists
            let default_gen_dir = params.base_dir.join("__generated__");
            if default_gen_dir.exists() && default_gen_dir.is_dir() {
                if let Err(e) = std::fs::remove_dir_all(&default_gen_dir) {
                    eprintln!(
                        "{}: {} - {}",
                        "Failed to remove default generated directory".red(),
                        default_gen_dir.display().to_string().red(),
                        e
                    );
                } else if verbose {
                    println!(
                        "{}: {}",
                        "Removed directory".bright_black(),
                        default_gen_dir.display().to_string().bright_black()
                    );
                }
            }
        }
    }

    Ok((Vec::new(), Vec::new()))
}

fn surgical_clean(dir: &Path, verbose: bool, entrypoint_name: &str) -> Result<(), ()> {
    let mut ok = true;

    let walker = ignore::WalkBuilder::new(dir)
        .hidden(false)
        .git_ignore(false)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.to_string_lossy().ends_with(".codegen.ts") {
            if let Err(e) = std::fs::remove_file(path) {
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

    let mut known_files = vec![
        format!("{}.ts", entrypoint_name),
        "graphql.ts".to_string(),
        "manifest.json".to_string(),
        "permissions.ts".to_string(),
        "fragment-masking.ts".to_string(),
        "index.ts".to_string(),
    ];
    known_files.sort();
    known_files.dedup();
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

    let mut should_write = true;
    if let Ok(metadata) = std::fs::metadata(&abs_out_path)
        && metadata.len() == ts_code.len() as u64
        && let Ok(existing) = std::fs::read(&abs_out_path)
        && existing == ts_code.as_bytes()
    {
        should_write = false;
    }

    if should_write {
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
    }
    Ok((ops, frags))
}
