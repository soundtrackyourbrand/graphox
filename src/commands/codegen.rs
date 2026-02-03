use apollo_compiler::{executable, Node, Schema};
use fnv::{FnvHashMap as HashMap, FnvHashSet as HashSet};
use graphql_rust::config::{Config, SchemaSource};
use graphql_rust::engine::{Engine, FragmentMetadata};
use std::path::{Path, PathBuf};

struct CodegenParams<'a> {
    base_dir: &'a Path,
    source: &'a SchemaSource,
    include_patterns: &'a [String],
    exclude_patterns: &'a [String],
    output_dir: Option<&'a str>,
    scalars: &'a Option<HashMap<String, String>>,
    schema_import: &'a Option<String>,
    fragment_to_path: &'a HashMap<String, String>,
    fragment_to_import: &'a HashMap<String, String>,
    all_fragments: &'a HashMap<String, Node<executable::Fragment>>,
    global_metadata: &'a [FragmentMetadata],
    generate_ast_for_fragments: bool,
}

pub async fn run_codegen(
    config: Option<Config>,
    schema_path: &str,
    scan_path: &str,
    output_dir: Option<&str>,
    watch: bool,
    clean: bool,
) {
    if !watch {
        if !execute_codegen(config, schema_path, scan_path, output_dir, clean).await {
            std::process::exit(1);
        }
        return;
    }

    println!("Watching for changes...");
    let _ = execute_codegen(config.clone(), schema_path, scan_path, output_dir, false).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    let mut debouncer = notify_debouncer_mini::new_debouncer(
        std::time::Duration::from_millis(200),
        move |res: notify_debouncer_mini::DebounceEventResult| match res {
            Ok(_) => {
                let _ = tx.blocking_send(());
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        },
    )
    .expect("Failed to create debouncer");

    debouncer
        .watcher()
        .watch(Path::new(scan_path), notify::RecursiveMode::Recursive)
        .expect("Failed to watch directory");

    if let Some(cfg) = &config {
        for project in &cfg.projects {
            for file in project.schema.files() {
                debouncer
                    .watcher()
                    .watch(&cfg.base_dir.join(file), notify::RecursiveMode::NonRecursive)
                    .ok();
            }
        }
        if let Some(schema_types) = &cfg.schema_types {
            for st in schema_types {
                for file in st.schema.files() {
                    debouncer
                        .watcher()
                        .watch(&cfg.base_dir.join(file), notify::RecursiveMode::NonRecursive)
                        .ok();
                }
            }
        }
    } else {
        debouncer
            .watcher()
            .watch(Path::new(schema_path), notify::RecursiveMode::NonRecursive)
            .expect("Failed to watch schema");
    }

    while rx.recv().await.is_some() {
        println!("\nChange detected, re-running codegen...");
        let _ = execute_codegen(config.clone(), schema_path, scan_path, output_dir, false).await;
    }
}

async fn execute_codegen(
    config: Option<Config>,
    schema_path: &str,
    scan_path: &str,
    output_dir: Option<&str>,
    clean: bool,
) -> bool {
    let mut success = true;
    let mut all_generated_operations = Vec::new();

    if let Some(cfg) = &config {
        let workspace_metadata = Engine::scan_workspace(cfg);
        let global_metadata = &workspace_metadata.fragments;

        let all_graphql_paths: Vec<_> = global_metadata
            .iter()
            .map(|m| PathBuf::from(&m.path))
            .collect();

        let global_output_dir = output_dir.or(cfg.output_dir.as_deref());
        for project in &cfg.projects {
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
            let project_files = graphql_rust::utils::get_project_files(&abs_includes, &abs_excludes);
            let project_files_set: HashSet<String> = project_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            println!("Processing project with schema: {}", project.schema.as_key());
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

            let schema = match Engine::load_schema(&cfg.base_dir, &project.schema) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", e);
                    success = false;
                    continue;
                }
            };
            let valid_schema = match schema.clone().validate() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "Schema validation failed for {}: {}",
                        project.schema.as_key(),
                        e
                    );
                    success = false;
                    continue;
                }
            };

            let all_fragments = Engine::resolve_fragments(&valid_schema, &all_graphql_paths);

            let mut fragment_to_path: HashMap<String, String> = HashMap::default();
            let mut fragment_to_import: HashMap<String, String> = HashMap::default();

            for meta in global_metadata {
                let is_local = project_files_set.contains(&meta.path);

                if is_local {
                    fragment_to_path.insert(meta.name.clone(), meta.path.clone());
                    if let Some(a) = &meta.import_alias {
                        fragment_to_import.insert(meta.name.clone(), a.clone());
                    }
                } else if meta.is_public {
                    fragment_to_path
                        .entry(meta.name.clone())
                        .or_insert_with(|| meta.path.clone());
                    if let Some(a) = &meta.import_alias {
                        fragment_to_import
                            .entry(meta.name.clone())
                            .or_insert_with(|| a.clone());
                    }
                }
            }

            match execute_project_codegen_entry(
                CodegenParams {
                    base_dir: &cfg.base_dir,
                    source: &project.schema,
                    include_patterns: &abs_includes,
                    exclude_patterns: &abs_excludes,
                    output_dir: project_output_dir,
                    scalars: &cfg.scalars,
                    schema_import: &schema_import,
                    fragment_to_path: &fragment_to_path,
                    fragment_to_import: &fragment_to_import,
                    all_fragments: &all_fragments,
                    global_metadata: &global_metadata,
                    generate_ast_for_fragments: cfg.generate_ast_for_fragments.unwrap_or(false),
                },
                clean,
            )
            .await
            {
                Ok(ops) => all_generated_operations.extend(ops),
                Err(_) => success = false,
            }
        }

        if let Some(schema_types) = &cfg.schema_types {
            for st in schema_types {
                let abs_output = cfg.base_dir.join(&st.output);
                if clean {
                    if abs_output.exists() {
                        if let Err(e) = std::fs::remove_file(&abs_output) {
                            eprintln!("Failed to remove {}: {}", abs_output.display(), e);
                            success = false;
                        } else {
                            println!("Removed: {}", abs_output.display());
                        }
                    }
                } else {
                    println!("Generating types for schema: {}", st.schema.as_key());
                    if !execute_schema_codegen(
                        &cfg.base_dir,
                        &st.schema,
                        &abs_output.to_string_lossy(),
                        &cfg.scalars,
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
                    println!("Generating entrypoint: {}", entrypoint_path.display());
                    let content = graphql_rust::features::codegen::generate_entrypoint_content(
                        &out_dir_path,
                        &all_generated_operations,
                    );
                    if let Err(e) = std::fs::write(&entrypoint_path, content) {
                        eprintln!("Failed to write entrypoint: {}", e);
                        success = false;
                    }
                }
            }
        }
    } else {
        let fragment_map = Engine::scan_path(scan_path);
        let all_graphql_paths: Vec<_> = fragment_map.values().map(PathBuf::from).collect();

        let schema_text = std::fs::read_to_string(schema_path).unwrap_or_default();
        let schema = match Schema::parse(&schema_text, schema_path) {
            Ok(s) => s,
            Err(e) => {
                if !clean {
                    eprintln!("Failed to parse schema {}: {}", schema_path, e);
                    return false;
                }
                Schema::parse("type Query { _empty: String }", "empty.graphql").unwrap()
            }
        };

        match schema.validate() {
            Ok(valid_schema) => {
                let all_fragments = Engine::resolve_fragments(&valid_schema, &all_graphql_paths);
                let include_glob = if std::path::Path::new(scan_path).is_file() {
                    scan_path.to_string()
                } else if scan_path == "." || scan_path == "./" {
                    "**/*".to_string()
                } else {
                    format!("{}/**/*", scan_path)
                };
                if execute_project_codegen_entry(
                    CodegenParams {
                        base_dir: Path::new("."),
                        source: &SchemaSource::Single(schema_path.to_string()),
                        include_patterns: &[include_glob],
                        exclude_patterns: &[],
                        output_dir,
                        scalars: &None,
                        schema_import: &None,
                        fragment_to_path: &fragment_map,
                        fragment_to_import: &HashMap::default(),
                        all_fragments: &all_fragments,
                        global_metadata: &[],
                        generate_ast_for_fragments: false,
                    },
                    clean,
                )
                .await
                .is_err()
                {
                    success = false;
                }
            }
            Err(e) => {
                if !clean {
                    eprintln!("Schema validation failed for {}: {}", schema_path, e);
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
) -> bool {
    let schema = match Engine::load_schema(base_dir, source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            return false;
        }
    };

    let ts_code = graphql_rust::features::codegen::generate_schema_types(&schema, scalars);
    let out_path = Path::new(output_path);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Err(e) = std::fs::write(out_path, ts_code) {
        eprintln!("Failed to write schema types file {}: {}", output_path, e);
        return false;
    }
    println!("Generated schema types: {}", out_path.display());
    true
}

async fn execute_project_codegen_entry(
    params: CodegenParams<'_>,
    clean: bool,
) -> Result<Vec<graphql_rust::features::codegen::OperationGenerated>, ()> {
    let mut success = true;
    let mut generated_ops = Vec::new();
    if !clean {
        let schema = match Engine::load_schema(params.base_dir, params.source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", e);
                return Err(());
            }
        };

        let paths =
            graphql_rust::utils::get_project_files(params.include_patterns, params.exclude_patterns);

        let mut docs = Vec::new();
        for path in paths {
            if let Some(doc) = Engine::parse_doc(&path)
                && !doc.get_graphql_trees().is_empty()
            {
                docs.push((path, doc));
            }
        }

        for (path, doc) in &docs {
            let ctx = graphql_rust::features::codegen::CodegenContext {
                schema: &schema,
                fragment_to_path: params.fragment_to_path,
                fragment_to_import: params.fragment_to_import,
                all_fragments: params.all_fragments,
                current_file_path: path,
                scalars: params.scalars,
                schema_import: params.schema_import,
                generate_ast_for_fragments: params.generate_ast_for_fragments,
            };

            match execute_single_file_codegen(doc, &ctx, params.output_dir, params.base_dir) {
                Ok(mut ops) => {
                    generated_ops.append(&mut ops);
                }
                Err(e) => {
                    if !e.contains("No executable operations") {
                        eprintln!("Error generating types for {}: {}", path.display(), e);
                        success = false;

                        if e.contains("Fragment") && e.contains("not found") {
                            for meta in params.global_metadata {
                                if !meta.is_public && e.contains(&format!("'{}'", meta.name)) {
                                    eprintln!(
                                    "  Hint: Fragment '{}' exists in {} but is not marked as @public",
                                    meta.name, meta.path
                                );
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Clean mode
        let paths =
            graphql_rust::utils::get_project_files(params.include_patterns, params.exclude_patterns);
        for path in paths {
            let out_path = graphql_rust::utils::get_output_path(&path, params.output_dir);
            if out_path.exists() {
                if let Err(e) = std::fs::remove_file(&out_path) {
                    eprintln!("Failed to remove {}: {}", out_path.display(), e);
                    success = false;
                } else {
                    println!("Removed: {}", out_path.display());
                }
            }
        }

        if let Some(out_dir) = params.output_dir {
            let entrypoint_path = params.base_dir.join(out_dir).join("graphql.ts");
            if entrypoint_path.exists() {
                if let Err(e) = std::fs::remove_file(&entrypoint_path) {
                    eprintln!(
                        "Failed to remove entrypoint {}: {}",
                        entrypoint_path.display(),
                        e
                    );
                    success = false;
                } else {
                    println!("Removed: {}", entrypoint_path.display());
                }
            }
        }
    }
    if success {
        Ok(generated_ops)
    } else {
        Err(())
    }
}

fn execute_single_file_codegen(
    doc: &graphql_rust::DocumentState,
    ctx: &graphql_rust::features::codegen::CodegenContext<'_>,
    output_dir: Option<&str>,
    base_dir: &Path,
) -> Result<Vec<graphql_rust::features::codegen::OperationGenerated>, String> {
    let (ts_code, mut ops) = graphql_rust::features::codegen::generate_typescript(doc, ctx)?;
    let out_path_raw =
        graphql_rust::utils::get_output_path(doc.uri.to_file_path().unwrap().as_path(), output_dir);

    let abs_out_path = if out_path_raw.is_absolute() {
        out_path_raw
    } else {
        base_dir.join(out_path_raw)
    };

    for op in &mut ops {
        op.codegen_path = abs_out_path.clone();
    }

    if let Some(parent) = abs_out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&abs_out_path, ts_code).map_err(|e| e.to_string())?;
    println!("Generated: {}", abs_out_path.display());
    Ok(ops)
}
