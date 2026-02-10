//! Codegen runner module
//!
//! This module handles running the TypeScript code generation process,
//! processing each project, generating types, and creating the entrypoint file.

use graphox_core::Config;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::MessageType;

/// Runs the codegen process for all projects in the configuration
pub async fn run_codegen(
    client: Client,
    config: Config,
    type_caches: Arc<dashmap::DashMap<String, Arc<graphox_codegen::TypeCache>, ahash::RandomState>>,
    supports_progress: bool,
) {
    // Create progress reporter
    let progress = super::progress::ProgressReporter::new(
        client.clone(),
        "Generating TypeScript types",
        supports_progress,
    )
    .await;

    progress.report("Scanning workspace...", Some(5)).await;

    let workspace_metadata = graphox_core::engine::Engine::scan_workspace(
        &config,
        tower_lsp::lsp_types::PositionEncodingKind::UTF8,
        None,
    );

    let global_metadata = &workspace_metadata.fragments;

    // Report progress
    let total_projects = config
        .projects
        .iter()
        .filter(|p| p.codegen_enabled())
        .count();
    let mut current_project = 0;
    let mut all_generated_operations = Vec::new();

    // Generate types for each project
    for (project, project_meta) in config.projects.iter().zip(&workspace_metadata.projects) {
        // Skip projects with codegen disabled
        if !project.codegen_enabled() {
            continue;
        }

        current_project += 1;
        let project_files = &project_meta.files;
        let project_output_dir = project.output_dir.as_deref();

        progress
            .report(
                format!(
                    "Processing project {}/{}...",
                    current_project, total_projects
                ),
                Some(5 + (current_project * 70 / total_projects) as u32),
            )
            .await;

        let project_schema_files: ahash::AHashSet<_> = project.schema.files().into_iter().collect();
        let schema_import = config.schema_types.as_ref().and_then(|sts| {
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

        let schema = match graphox_core::schema::load_schema(&config.base_dir, &project.schema) {
            Ok(s) => s,
            Err(e) => {
                client
                    .log_message(MessageType::ERROR, format!("Failed to load schema: {}", e))
                    .await;
                continue;
            }
        };

        let valid_schema = match schema.validate() {
            Ok(v) => v,
            Err(e) => {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Schema validation failed for project {}: {}",
                            project.include.as_key(),
                            e
                        ),
                    )
                    .await;
                continue;
            }
        };

        let project_context = graphox_core::engine::Engine::resolve_project_context(
            &valid_schema,
            global_metadata,
            project_files,
        );

        // Get or create persistent type cache for this schema
        let schema_key = project.schema.as_key();
        let type_cache = type_caches
            .entry(schema_key.clone())
            .or_insert_with(|| Arc::new(graphox_codegen::TypeCache::new()))
            .clone();

        let total_files = project_files.len();
        let mut current_file = 0;

        for path in project_files {
            current_file += 1;

            if let Some(doc) = workspace_metadata.documents.get(path) {
                if doc.get_graphql_trees().is_empty() {
                    continue;
                }

                let ctx = graphox_codegen::CodegenContext::new(
                    &valid_schema,
                    &project_context.fragment_to_path,
                    &project_context.fragment_to_import,
                    &project_context.fragment_to_type_only,
                    &project_context.all_fragments,
                    path,
                    &config.scalars,
                    &schema_import,
                    config.generate_ast_for_fragments.unwrap_or(false),
                    &project_context.fragment_dependencies,
                    &type_cache, // Use persistent cache from Backend
                    project
                        .document_suffix
                        .as_deref()
                        .or(config.document_suffix.as_deref())
                        .unwrap_or("Document"),
                    project
                        .variables_suffix
                        .as_deref()
                        .or(config.variables_suffix.as_deref())
                        .unwrap_or("Variables"),
                    project
                        .fragment_suffix
                        .as_deref()
                        .or(config.fragment_suffix.as_deref())
                        .unwrap_or(""),
                    graphox_codegen::FragmentMasking::from_config(
                        &project
                            .fragment_masking
                            .clone()
                            .or(config.fragment_masking.clone()),
                    ),
                    {
                        if let Some(out_dir) = project_output_dir {
                            let out_path = graphox_core::utils::get_output_path(
                                path,
                                &config.base_dir,
                                project_output_dir,
                                Some(
                                    graphox_core::utils::get_glob_root(&project.include.as_key())
                                        .to_str()
                                        .unwrap_or(""),
                                ),
                            );
                            let abs_out_dir = if out_path.is_absolute() {
                                out_path.parent().unwrap().to_path_buf()
                            } else {
                                config
                                    .base_dir
                                    .join(out_path)
                                    .parent()
                                    .unwrap()
                                    .to_path_buf()
                            };

                            let abs_masking_dir = config.base_dir.join(out_dir);
                            let rel_to_masking =
                                pathdiff::diff_paths(&abs_masking_dir, &abs_out_dir)
                                    .unwrap_or_else(|| PathBuf::from("."));

                            let mut path_str = graphox_core::utils::to_posix_path(
                                &rel_to_masking.join("fragment-masking"),
                            );
                            if !path_str.starts_with('.') && !path_str.starts_with('/') {
                                path_str.insert_str(0, "./");
                            }
                            path_str
                        } else {
                            "./fragment-masking".to_string()
                        }
                    },
                );

                if let Ok((ts_code, mut ops)) = graphox_codegen::generate_typescript(doc, &ctx) {
                    let glob_pattern = project.include.patterns().first().cloned();
                    let include_prefix = glob_pattern
                        .as_ref()
                        .map(|p| graphox_core::utils::get_glob_root(p));
                    let out_path = graphox_core::utils::get_output_path(
                        path,
                        &config.base_dir,
                        project_output_dir,
                        include_prefix.as_ref().and_then(|p| p.to_str()),
                    );
                    let abs_out_path = if out_path.is_absolute() {
                        out_path
                    } else {
                        config.base_dir.join(out_path)
                    };

                    if let Some(parent) = abs_out_path.parent()
                        && let Err(e) = std::fs::create_dir_all(parent)
                    {
                        client
                            .log_message(
                                MessageType::ERROR,
                                format!(
                                    "Failed to create output directory {}: {}",
                                    parent.display(),
                                    e
                                ),
                            )
                            .await;
                        continue;
                    }

                    match std::fs::write(&abs_out_path, ts_code) {
                        Ok(_) => {
                            for op in &mut ops {
                                op.codegen_path = abs_out_path.clone();
                            }
                            all_generated_operations.extend(ops);
                        }
                        Err(e) => {
                            client
                                .log_message(
                                    MessageType::ERROR,
                                    format!(
                                        "Failed to write generated types to {}: {}",
                                        abs_out_path.display(),
                                        e
                                    ),
                                )
                                .await;
                        }
                    }
                }
            }

            // Report progress every 10 files to avoid too many notifications
            if current_file % 10 == 0 || current_file == total_files {
                let project_percentage = 5 + (current_project * 70 / total_projects) as u32;
                let file_percentage = if total_files > 0 {
                    (current_file * 70 / total_projects / total_files) as u32
                } else {
                    0
                };
                progress
                    .report(
                        format!("Generating types ({}/{})", current_file, total_files),
                        Some(project_percentage + file_percentage),
                    )
                    .await;
            }
        }
    }

    progress
        .report("Writing entrypoint file...", Some(80))
        .await;

    if let Some(out_dir) = config.projects.first().and_then(|p| p.output_dir.as_ref()) {
        let out_dir_path = config.base_dir.join(out_dir);
        let entrypoint_path = out_dir_path.join("graphql.ts");
        let fragment_masking =
            graphox_codegen::FragmentMasking::from_config(&config.fragment_masking);
        if !all_generated_operations.is_empty() {
            let content = graphox_codegen::generate_entrypoint_content(
                &out_dir_path,
                &all_generated_operations,
                config.document_suffix(),
                config.variables_suffix(),
                &fragment_masking,
            );
            std::fs::create_dir_all(&out_dir_path).ok();
            if let Err(e) = std::fs::write(&entrypoint_path, content) {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Failed to write entrypoint file {}: {}",
                            entrypoint_path.display(),
                            e
                        ),
                    )
                    .await;
            }

            if fragment_masking.is_enabled() {
                let masking_path = out_dir_path.join("fragment-masking.ts");
                let masking_content = graphox_codegen::generate_fragment_masking_file(
                    fragment_masking.unmask_function_name(),
                );
                if let Err(e) = std::fs::write(&masking_path, masking_content) {
                    client
                        .log_message(
                            MessageType::ERROR,
                            format!(
                                "Failed to write fragment-masking file {}: {}",
                                masking_path.display(),
                                e
                            ),
                        )
                        .await;
                }
            }

            let manifest_path = out_dir_path.join("manifest.json");
            let manifest_entries: Vec<_> = all_generated_operations
                .iter()
                .map(|op| {
                    let rel_path = pathdiff::diff_paths(&op.codegen_path, &out_dir_path)
                        .unwrap_or_else(|| op.codegen_path.clone());
                    let mut path_str = graphox_core::utils::to_posix_path(&rel_path);
                    if !path_str.starts_with('.') && !path_str.starts_with('/') {
                        path_str = format!("./{}", path_str);
                    }
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

            if let Ok(manifest_json) = serde_json::to_string_pretty(&manifest_entries)
                && let Err(e) = std::fs::write(&manifest_path, manifest_json)
            {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Failed to write manifest file {}: {}",
                            manifest_path.display(),
                            e
                        ),
                    )
                    .await;
            }
        }
    }

    progress
        .end(Some(format!(
            "Generated types for {} projects",
            total_projects
        )))
        .await;
}
