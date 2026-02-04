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

    println!("Watching for changes...");
    let _ = execute_codegen(config.clone(), output_dir, verbose, false).await;

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
        println!("\nChange detected, re-running codegen...");
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

        println!("Processing project: {}", project.include.as_key());
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
                    "Schema validation failed for project {}: {}",
                    project.include.as_key(),
                    e
                );
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
                    println!("Generating permissions: {}", permissions_path.display());
                }
                let content = graphql_rust::features::codegen::generate_permissions_content(
                    &valid_schema,
                    &cfg.scalars,
                    &schema_import,
                );
                if let Err(e) = std::fs::write(&permissions_path, content) {
                    eprintln!("Failed to write permissions: {}", e);
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
                    } else if verbose {
                        println!("Removed: {}", abs_output.display());
                    }
                }
            } else {
                println!("Generating types for schema: {}", st.output);
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
                    println!("Generating entrypoint: {}", entrypoint_path.display());
                }
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

    success
}

async fn execute_schema_codegen(
    base_dir: &Path,
    source: &SchemaSource,
    output_path: &str,
    scalars: &Option<HashMap<String, String>>,
    verbose: bool,
) -> bool {
    let schema = match Engine::load_schema(base_dir, source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            return false;
        }
    };
    let valid_schema = match schema.validate() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Schema validation failed for {}: {}", source.as_key(), e);
            return false;
        }
    };

    let ts_code = graphql_rust::features::codegen::generate_schema_types(&valid_schema, scalars);
    let out_path = Path::new(output_path);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Err(e) = std::fs::write(out_path, ts_code) {
        eprintln!("Failed to write schema types file {}: {}", output_path, e);
        return false;
    }
    if verbose {
        println!("Generated: {}", out_path.display());
    }
    true
}

async fn execute_project_codegen_entry(
    params: CodegenParams<'_>,
    verbose: bool,
    clean: bool,
) -> Result<Vec<graphql_rust::features::codegen::OperationGenerated>, ()> {
    if !clean {
        let schema = match Engine::load_schema(params.base_dir, params.source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", e);
                return Err(());
            }
        };
        let valid_schema = match schema.validate() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Schema validation failed: {}", e);
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
                        eprintln!("Error generating types for {}: {}", path.display(), e);
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
                        eprintln!("Failed to remove {}: {}", out_path.display(), e);
                        ok = false;
                    } else if verbose {
                        println!("Removed: {}", out_path.display());
                    }
                }
                ok
            })
            .reduce(|| true, |a, b| a && b);

        let mut entrypoint_ok = true;
        let mut permissions_ok = true;
        if let Some(out_dir) = params.output_dir {
            let entrypoint_path = params.base_dir.join(out_dir).join("graphql.ts");
            if entrypoint_path.exists() {
                if let Err(e) = std::fs::remove_file(&entrypoint_path) {
                    eprintln!(
                        "Failed to remove entrypoint {}: {}",
                        entrypoint_path.display(),
                        e
                    );
                    entrypoint_ok = false;
                } else if verbose {
                    println!("Removed: {}", entrypoint_path.display());
                }
            }

            if params.generate_permissions {
                let permissions_path = params.base_dir.join(out_dir).join("permissions.ts");
                if permissions_path.exists() {
                    if let Err(e) = std::fs::remove_file(&permissions_path) {
                        eprintln!(
                            "Failed to remove permissions {}: {}",
                            permissions_path.display(),
                            e
                        );
                        permissions_ok = false;
                    } else if verbose {
                        println!("Removed: {}", permissions_path.display());
                    }
                }
            }
        }

        if success && entrypoint_ok && permissions_ok {
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
        println!("Generated: {}", abs_out_path.display());
    }
    Ok(ops)
}
