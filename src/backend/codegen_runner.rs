//! Codegen runner module
//!
//! This module handles running the TypeScript code generation process,
//! processing each project, generating types, and creating the entrypoint file.

use crate::Config;
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::MessageType;

/// Runs the codegen process for all projects in the configuration
pub async fn run_codegen(
    client: Client,
    config: Config,
    type_caches: Arc<
        dashmap::DashMap<String, Arc<crate::features::codegen::TypeCache>, ahash::RandomState>,
    >,
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

    let workspace_metadata = crate::engine::Engine::scan_workspace(&config);

    let global_metadata = &workspace_metadata.fragments;
    let global_output_dir = config.output_dir.as_deref();

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
        let project_output_dir = project.output_dir.as_deref().or(global_output_dir);

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

        let schema = match crate::schema::load_schema(&config.base_dir, &project.schema) {
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

        let project_context = crate::engine::Engine::resolve_project_context(
            &valid_schema,
            global_metadata,
            project_files,
        );

        // Get or create persistent type cache for this schema
        let schema_key = project.schema.as_key();
        let type_cache = type_caches
            .entry(schema_key.clone())
            .or_insert_with(|| Arc::new(crate::features::codegen::TypeCache::new()))
            .clone();

        let total_files = project_files.len();
        let mut current_file = 0;

        for path in project_files {
            current_file += 1;

            if let Some(doc) = workspace_metadata.documents.get(path) {
                if doc.get_graphql_trees().is_empty() {
                    continue;
                }

                let ctx = crate::features::codegen::CodegenContext::new(
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
                );

                if let Ok((ts_code, mut ops)) =
                    crate::features::codegen::generate_typescript(doc, &ctx)
                {
                    let out_path =
                        crate::utils::get_output_path(path, &config.base_dir, project_output_dir);
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

    if let Some(out_dir) = global_output_dir {
        let out_dir_path = config.base_dir.join(out_dir);
        let entrypoint_path = out_dir_path.join("graphql.ts");
        if !all_generated_operations.is_empty() {
            let content = crate::features::codegen::generate_entrypoint_content(
                &out_dir_path,
                &all_generated_operations,
            );
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
        }
    }

    progress
        .end(Some(format!(
            "Generated types for {} projects",
            total_projects
        )))
        .await;
}
