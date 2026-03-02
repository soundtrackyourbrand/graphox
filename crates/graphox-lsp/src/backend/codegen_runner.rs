//! Codegen runner module
//!
//! This module handles running the TypeScript code generation process,
//! processing each project, generating types, and creating the entrypoint file.

use graphox_core::config::Config;
use graphox_core::types::{DocumentsMap, FragmentDefsMap};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::MessageType;

/// Runs the codegen process for specified projects or all projects
pub async fn run_codegen(
    client: Client,
    config: Config,
    type_caches: Arc<
        dashmap::DashMap<String, Arc<graphox_codegen::SchemaAnalysisCaches>, ahash::RandomState>,
    >,
    documents: DocumentsMap,
    fragment_defs: FragmentDefsMap,
    supports_progress: bool,
    projects_to_run: Option<HashSet<String>>,
) {
    // Create progress reporter
    let progress = super::progress::ProgressReporter::new(
        client.clone(),
        "Generating TypeScript types",
        supports_progress,
    )
    .await;

    progress
        .report("Preparing codegen metadata...", Some(5))
        .await;

    // Build global metadata from existing fragment_defs instead of re-scanning disk
    let mut global_metadata = Vec::new();
    for entry in fragment_defs.iter() {
        let uri = entry.key();
        let frags = entry.value();

        let import_path = if let Ok(p) = uri.to_file_path() {
            config
                .get_project_for_path(&p)
                .and_then(|proj| proj.import().map(|s| s.to_string()))
        } else {
            None
        };

        for frag in frags {
            global_metadata.push(graphox_core::engine::FragmentMetadata {
                name: frag.name.clone(),
                path: Arc::from(uri.to_string()),
                import_alias: import_path.as_deref().map(Arc::from),
                is_public: frag.is_public,
                is_type_only: frag.is_type_only,
                masked_source: Arc::from(""), // Not needed for project context resolution
                direct_deps: frag.used_fragments.clone(),
                transitive_deps: frag.transitive_deps.clone(),
                type_fields: frag.type_fields.clone(),
            });
        }
    }

    // Identify which projects to run
    let projects_configs: Vec<_> = config
        .projects()
        .iter()
        .filter(|p| {
            if !config.get_project_codegen_enabled(p) {
                return false;
            }
            if let Some(to_run) = &projects_to_run {
                to_run.contains(&p.include().as_key())
            } else {
                true
            }
        })
        .collect();

    let total_projects = projects_configs.len();
    if total_projects == 0 {
        progress
            .end(Some("No projects require codegen".to_string()))
            .await;
        return;
    }

    let mut project_operations_list = Vec::with_capacity(total_projects);
    let mut project_fragments_list = Vec::with_capacity(total_projects);

    // Generate types for each project
    for (idx, project) in projects_configs.iter().enumerate() {
        let current_project = idx + 1;

        // Find files for this project from our existing documents map
        let project_files: Vec<PathBuf> = documents
            .iter()
            .filter_map(|entry| {
                let uri = entry.key();
                if let Ok(path) = uri.to_file_path() {
                    let rel_path = path.strip_prefix(config.base_dir()).unwrap_or(&path);

                    let include_match = project.include().is_match(rel_path);
                    let exclude_match = project
                        .exclude()
                        .as_ref()
                        .is_some_and(|e| e.is_match(rel_path));

                    if include_match && !exclude_match {
                        return Some(path);
                    }
                }
                None
            })
            .collect();

        if project_files.is_empty() {
            project_operations_list.push(Vec::new());
            project_fragments_list.push(Vec::new());
            continue;
        }

        let project_output_dir = project.output_dir();

        progress
            .report(
                format!(
                    "Processing project {}/{}...",
                    current_project, total_projects
                ),
                Some(5 + (current_project * 70 / total_projects) as u32),
            )
            .await;

        let mut type_imports = ahash::AHashMap::default();
        let project_schema_files: ahash::AHashSet<_> =
            project.schema().files().into_iter().collect();
        let schema_import = if let Some(si) = project.codegen().schema_import() {
            Some(si.to_string())
        } else if config.schema_types().is_empty() {
            None
        } else {
            let mut matches: Vec<_> = config
                .schema_types()
                .iter()
                .filter(|st| {
                    let st_files = st.schema().files();
                    st_files.iter().all(|f| project_schema_files.contains(f))
                })
                .collect();

            matches.sort_by_key(|st| std::cmp::Reverse(st.schema().files().len()));

            // Build type_imports
            for st in matches.iter().rev() {
                if let Some(import_path) = st.import()
                    && let Ok(st_schema) =
                        graphox_core::schema::load_schema(config.base_dir(), st.schema())
                {
                    for type_name in st_schema.types.keys() {
                        type_imports.insert(type_name.to_string(), import_path.to_string());
                    }
                }
            }

            matches.first().and_then(|st| st.import().map(String::from))
        };

        let schema = match graphox_core::schema::load_schema(config.base_dir(), project.schema()) {
            Ok(s) => s,
            Err(e) => {
                client
                    .log_message(MessageType::ERROR, format!("Failed to load schema: {}", e))
                    .await;
                project_operations_list.push(Vec::new());
                project_fragments_list.push(Vec::new());
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
                            project.include().as_key(),
                            e
                        ),
                    )
                    .await;
                project_operations_list.push(Vec::new());
                project_fragments_list.push(Vec::new());
                continue;
            }
        };

        let project_context = graphox_core::engine::Engine::resolve_project_context(
            &valid_schema,
            &global_metadata,
            &project_files,
        );

        // Get or create persistent type cache for this schema
        let schema_key = project.schema().as_key();
        let type_cache = type_caches
            .entry(schema_key.clone())
            .or_insert_with(|| Arc::new(graphox_codegen::SchemaAnalysisCaches::new()))
            .clone();

        let mut project_ops = Vec::new();
        let mut project_frags: Vec<graphox_codegen::FragmentGenerated> = Vec::new();

        // Use DashMap for thread-safe result collection with per-key locking
        let project_results: dashmap::DashMap<
            PathBuf,
            (
                Vec<graphox_codegen::OperationGenerated>,
                Vec<graphox_codegen::FragmentGenerated>,
            ),
        > = dashmap::DashMap::new();

        // Process files in parallel with immediate writes
        project_files.par_iter().for_each(|path| {
            let uri = match tower_lsp::lsp_types::Url::from_file_path(path) {
                Ok(u) => u,
                Err(_) => {
                    log::warn!("Failed to convert path to URL: {:?}", path);
                    return;
                }
            };
            let doc_ref = match documents.get(&uri) {
                Some(doc) if !doc.get_graphql_trees().is_empty() => doc,
                _ => return,
            };
            let doc = doc_ref.value().clone();

            let include_prefix_path = project
                .include()
                .patterns()
                .iter()
                .map(|p| graphox_core::utils::get_glob_root(p))
                .find(|root| {
                    let abs_root = config.base_dir().join(root);
                    let abs_root = std::fs::canonicalize(&abs_root).unwrap_or(abs_root);
                    graphox_core::utils::path_starts_with(path, &abs_root)
                });
            let out_path = graphox_core::utils::get_output_path(
                path,
                config.base_dir(),
                project_output_dir.map(Path::new),
                include_prefix_path.as_deref(),
            );
            let abs_out_path = if out_path.is_absolute() {
                out_path
            } else {
                config.base_dir().join(out_path)
            };
            let codegen_path = abs_out_path.clone();

            let codegen_config = config.get_codegen_config(Some(project));

            let ctx = graphox_codegen::CodegenContext::new(
                &valid_schema,
                &project_context.fragment_to_path,
                &project_context.fragment_to_import,
                &project_context.fragment_to_type_only,
                &project_context.all_fragments,
                path,
                config.scalars(),
                &schema_import,
                &type_imports,
                codegen_config.generate_ast_for_fragments(),
                &project_context.fragment_dependencies,
                &type_cache,
                &codegen_config,
                {
                    if let Some(out_dir) = project_output_dir {
                        let out_path = graphox_core::utils::get_output_path(
                            path,
                            config.base_dir(),
                            project_output_dir.map(Path::new),
                            include_prefix_path.as_deref(),
                        );
                        let abs_out_dir = if out_path.is_absolute() {
                            out_path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| out_path.clone())
                        } else {
                            let joined = config.base_dir().join(&out_path);
                            joined
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| joined)
                        };

                        let abs_masking_dir = config.base_dir().join(out_dir);
                        let rel_to_masking = pathdiff::diff_paths(&abs_masking_dir, &abs_out_dir)
                            .unwrap_or_else(|| PathBuf::from("."));

                        let mut path_str = graphox_core::utils::to_posix_path(
                            &rel_to_masking.join("fragment-masking"),
                        );
                        if !path_str.starts_with('.') && !path_str.starts_with('/') {
                            path_str.insert_str(0, "./");
                        }
                        path_str.push_str(codegen_config.emit_extensions().as_str());
                        path_str
                    } else {
                        let mut path_str = "./fragment-masking".to_string();
                        path_str.push_str(codegen_config.emit_extensions().as_str());
                        path_str
                    }
                },
                codegen_path,
            );

            let result = graphox_codegen::generate_typescript(&doc, &ctx);
            let Ok((ts_code, mut ops, mut frags)) = result else {
                return;
            };

            // Write file only if changed
            let mut should_write = true;
            if abs_out_path.exists()
                && let Ok(existing) = std::fs::read_to_string(&abs_out_path)
                && existing == ts_code
            {
                should_write = false;
            }

            if should_write {
                if let Some(parent) = abs_out_path.parent()
                    && std::fs::create_dir_all(parent).is_err()
                {
                    return;
                }
                if std::fs::write(&abs_out_path, ts_code).is_err() {
                    return;
                }
            }

            // Update codegen_path and insert into shared DashMap
            for op in &mut ops {
                op.codegen_path = abs_out_path.clone();
            }
            for frag in &mut frags {
                frag.codegen_path = abs_out_path.clone();
            }
            project_results.insert(abs_out_path, (ops, frags));
        });

        // Collect results after parallel phase
        for entry in project_results.iter() {
            let (ops, frags) = entry.value();
            project_ops.extend(ops.clone());
            project_frags.extend(frags.clone());
        }

        project_operations_list.push(project_ops);
        project_fragments_list.push(project_frags);
    }

    progress
        .report("Writing entrypoint files...", Some(80))
        .await;

    // Group all generated operations by their canonicalized absolute output directory
    let mut dir_to_ops: std::collections::BTreeMap<
        PathBuf,
        Vec<graphox_codegen::OperationGenerated>,
    > = std::collections::BTreeMap::new();
    let mut dir_to_frags: std::collections::BTreeMap<
        PathBuf,
        Vec<graphox_codegen::FragmentGenerated>,
    > = std::collections::BTreeMap::new();
    let mut dir_to_config: ahash::AHashMap<PathBuf, graphox_core::config::CodegenConfig> =
        ahash::AHashMap::new();

    for ((project, project_ops), project_frags) in projects_configs
        .iter()
        .zip(project_operations_list)
        .zip(project_fragments_list)
    {
        let out_dir = project.output_dir().unwrap_or("__generated__");
        let out_dir_path = config.base_dir().join(out_dir);
        let canon_out_dir_path = out_dir_path
            .canonicalize()
            .unwrap_or_else(|_| out_dir_path.clone());

        dir_to_ops
            .entry(canon_out_dir_path.clone())
            .or_default()
            .extend(project_ops);

        dir_to_frags
            .entry(canon_out_dir_path.clone())
            .or_default()
            .extend(project_frags);

        if let std::collections::hash_map::Entry::Vacant(e) =
            dir_to_config.entry(canon_out_dir_path)
        {
            let codegen_config = config.get_codegen_config(Some(project));
            e.insert(codegen_config);
        }
    }

    for (out_dir_path, mut ops, mut frags) in dir_to_ops.into_iter().map(|(k, v)| {
        let frags = dir_to_frags.remove(&k).unwrap_or_default();
        (k, v, frags)
    }) {
        let codegen_config = dir_to_config.get(&out_dir_path).unwrap();

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

        let entrypoint_path = out_dir_path.join("graphql.ts");
        let content = graphox_codegen::generate_entrypoint_content(
            &out_dir_path,
            &ops,
            &frags,
            codegen_config,
            codegen_config.re_exports(),
            codegen_config.schema_import(),
        );

        let mut should_write_entry = true;
        if entrypoint_path.exists()
            && let Ok(existing) = std::fs::read_to_string(&entrypoint_path)
            && existing == content
        {
            should_write_entry = false;
        }

        if should_write_entry {
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
        }

        if codegen_config.fragment_masking_mode().is_enabled() {
            let masking_path = out_dir_path.join("fragment-masking.ts");
            let masking_content = graphox_codegen::generate_fragment_masking_file(
                codegen_config
                    .fragment_masking_mode()
                    .unmask_function_name(),
            );

            let mut should_write_masking = true;
            if masking_path.exists()
                && let Ok(existing) = std::fs::read_to_string(&masking_path)
                && existing == masking_content
            {
                should_write_masking = false;
            }

            if should_write_masking && let Err(e) = std::fs::write(&masking_path, masking_content) {
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

        let index_path = out_dir_path.join("index.ts");
        let index_content = graphox_codegen::generate_index_content(
            &graphox_codegen::FragmentMasking::from_core_config(&codegen_config.fragment_masking()),
            codegen_config.emit_extensions(),
        );

        let mut should_write_index = true;
        if index_path.exists()
            && let Ok(existing) = std::fs::read_to_string(&index_path)
            && existing == index_content
        {
            should_write_index = false;
        }

        if should_write_index && let Err(e) = std::fs::write(&index_path, index_content) {
            client
                .log_message(
                    MessageType::ERROR,
                    format!("Failed to write index.ts {}: {}", index_path.display(), e),
                )
                .await;
        }

        let manifest_path = out_dir_path.join("manifest.json");
        let generate_ast_for_frags = codegen_config.generate_ast_for_fragments();
        let manifest_entries: Vec<_> = ops
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
            .chain(
                frags
                    .iter()
                    .filter(|_| {
                        generate_ast_for_frags
                            || codegen_config.fragment_masking_mode().is_enabled()
                    })
                    .map(|frag| {
                        let rel_path = pathdiff::diff_paths(&frag.codegen_path, &out_dir_path)
                            .unwrap_or_else(|| frag.codegen_path.clone());
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
                            "source": frag.source_text,
                            "path": path_no_ext,
                            "name": frag.document_name
                        })
                    }),
            )
            .collect();

        if let Ok(manifest_json) = serde_json::to_string_pretty(&manifest_entries) {
            let mut should_write_manifest = true;
            if manifest_path.exists()
                && let Ok(existing) = std::fs::read_to_string(&manifest_path)
                && existing == manifest_json
            {
                should_write_manifest = false;
            }

            if should_write_manifest && let Err(e) = std::fs::write(&manifest_path, manifest_json) {
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
