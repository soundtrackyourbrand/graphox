//! Codegen runner module
//!
//! This module handles running the TypeScript code generation process,
//! processing each project, generating types, and creating the entrypoint file.

use crate::Config;
use tower_lsp::lsp_types::MessageType;
use tower_lsp::Client;

/// Runs the codegen process for all projects in the configuration
pub async fn run_codegen(client: Client, config: Config) {
    let workspace_metadata = crate::engine::Engine::scan_workspace(&config, |_, _| {});

    let global_metadata = &workspace_metadata.fragments;
    let global_output_dir = config.output_dir.as_deref();
    let mut all_generated_operations = Vec::new();

    for (project, project_meta) in config.projects.iter().zip(&workspace_metadata.projects) {
        let project_files = &project_meta.files;
        let project_output_dir = project.output_dir.as_deref().or(global_output_dir);

        let project_schema_files: fnv::FnvHashSet<_> =
            project.schema.files().into_iter().collect();
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
                let _ = client.log_message(MessageType::ERROR, e).await;
                continue;
            }
        };

        let valid_schema = match schema.validate() {
            Ok(v) => v,
            Err(e) => {
                let _ = client
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

        for path in project_files {
            if let Some(doc) = workspace_metadata.documents.get(path) {
                if doc.get_graphql_trees().is_empty() {
                    continue;
                }

                let ctx = crate::features::codegen::CodegenContext {
                    schema: &valid_schema,
                    fragment_to_path: &project_context.fragment_to_path,
                    fragment_to_import: &project_context.fragment_to_import,
                    fragment_to_type_only: &project_context.fragment_to_type_only,
                    all_fragments: &project_context.all_fragments,
                    current_file_path: path,
                    scalars: &config.scalars,
                    schema_import: &schema_import,
                    generate_ast_for_fragments: config.generate_ast_for_fragments.unwrap_or(false),
                };

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

                    if let Some(parent) = abs_out_path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }

                    if std::fs::write(&abs_out_path, ts_code).is_ok() {
                        for op in &mut ops {
                            op.codegen_path = abs_out_path.clone();
                        }
                        all_generated_operations.extend(ops);
                    }
                }
            }
        }
    }

    if let Some(out_dir) = global_output_dir {
        let out_dir_path = config.base_dir.join(out_dir);
        let entrypoint_path = out_dir_path.join("graphql.ts");
        if !all_generated_operations.is_empty() {
            let content = crate::features::codegen::generate_entrypoint_content(
                &out_dir_path,
                &all_generated_operations,
            );
            let _ = std::fs::write(entrypoint_path, content);
        }
    }
}
