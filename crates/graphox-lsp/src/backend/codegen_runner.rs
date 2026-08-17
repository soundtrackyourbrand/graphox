//! Codegen runner module
//!
//! This module handles running the TypeScript code generation process,
//! processing each project, generating types, and creating the entrypoint file.

use graphox_core::config::Config;
use graphox_core::types::DocumentsMap;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::lsp_types::{MessageType, Url};

type RunDocumentCache = HashMap<Url, Arc<graphox_core::DocumentState>>;

/// The workspace-wide codegen inputs that depend only on the set of files and their
/// fragment definitions: the global fragment metadata (for cross-project fragment
/// resolution) and the per-project file lists (from the filesystem walk). Both are
/// expensive to recompute (a filesystem walk per project + fragment extraction +
/// transitive-dependency computation across the whole workspace), so they are cached
/// and reused while the workspace is unchanged.
pub struct CodegenMetadata {
    pub global_metadata: Vec<graphox_core::engine::FragmentMetadata>,
    pub project_files_by_index: Vec<Vec<PathBuf>>,
}

/// Caches [`CodegenMetadata`] keyed by the workspace version. The key is sound
/// because the workspace version bumps on every change that can affect the file set
/// or any fragment definition (adds/removes, fragment edits) — while a pure
/// operation-body edit, the common case, leaves it untouched, so back-to-back
/// codegen runs for query edits reuse the cached walk + metadata instead of redoing
/// a full-workspace scan each time.
pub type CodegenMetadataCache = Arc<std::sync::RwLock<Option<(usize, Arc<CodegenMetadata>)>>>;

/// Parses every project file that is not already an in-memory document, so the
/// generation pass has a `DocumentState` for each. Used on a metadata cache hit,
/// where the (cached) metadata was built without producing a fresh document cache.
/// Only files absent from `documents` (e.g. opened-then-closed) are read from disk.
fn build_run_cache(
    project_files_by_index: &[Vec<PathBuf>],
    documents: &DocumentsMap,
    position_encoding: &tower_lsp::lsp_types::PositionEncodingKind,
) -> (RunDocumentCache, UnreadableFiles) {
    let mut run_cache = RunDocumentCache::new();
    let mut unreadable = UnreadableFiles::default();
    for project_files in project_files_by_index {
        for path in project_files {
            let _ = load_or_parse_document(
                path,
                documents,
                &mut run_cache,
                &mut unreadable,
                position_encoding,
            );
        }
    }
    (run_cache, unreadable)
}

/// Files that exist but could not be read this run. They look exactly like "has no
/// GraphQL" to the generation pass, so they are tracked separately: pruning must not
/// mistake a source it failed to read for one that was deleted.
type UnreadableFiles = ahash::AHashSet<PathBuf>;

fn load_or_parse_document(
    path: &Path,
    documents: &DocumentsMap,
    run_cache: &mut RunDocumentCache,
    unreadable: &mut UnreadableFiles,
    position_encoding: &tower_lsp::lsp_types::PositionEncodingKind,
) -> Option<Arc<graphox_core::DocumentState>> {
    let uri = Url::from_file_path(path).ok()?;

    if let Some(doc) = documents.get(&uri).map(|r| r.value().clone()) {
        return Some(doc);
    }

    if let Some(doc) = run_cache.get(&uri) {
        return Some(doc.clone());
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            // Still on disk, so this is a read failure rather than a deletion.
            if path.exists() {
                unreadable.insert(path.to_path_buf());
            }
            return None;
        }
    };
    if graphox_core::utils::has_generated_header(&content) {
        return None;
    }

    // Cheap pre-filter: a host-language file (.ts/.tsx/...) with no `gql`/`graphql`
    // marker cannot hold embedded GraphQL, so skip the expensive tree-sitter parse.
    // Without this, every cold codegen run tree-sitter-parses thousands of non-GraphQL
    // source files only to find no fragments (mirrors the workspace scan's pre-filter).
    if graphox_core::document::DocumentLanguage::from_uri(&uri).is_host_language() {
        let bytes = content.as_bytes();
        let has_gql = bytes.windows(3).any(|w| w.eq_ignore_ascii_case(b"gql"))
            || bytes.windows(7).any(|w| w.eq_ignore_ascii_case(b"graphql"));
        if !has_gql {
            return None;
        }
    }

    let doc = Arc::new(graphox_core::DocumentState::new_from_thread_local(
        uri.clone(),
        &content,
        position_encoding.clone(),
    ));
    run_cache.insert(uri, doc.clone());
    Some(doc)
}

fn get_document_for_codegen(
    path: &Path,
    documents: &DocumentsMap,
    run_cache: &RunDocumentCache,
) -> Option<Arc<graphox_core::DocumentState>> {
    let uri = Url::from_file_path(path).ok()?;

    documents
        .get(&uri)
        .map(|r| r.value().clone())
        .or_else(|| run_cache.get(&uri).cloned())
}

pub fn collect_codegen_metadata(
    config: &Config,
    documents: &DocumentsMap,
    position_encoding: &tower_lsp::lsp_types::PositionEncodingKind,
) -> (
    Vec<graphox_core::engine::FragmentMetadata>,
    Vec<Vec<PathBuf>>,
    RunDocumentCache,
    UnreadableFiles,
) {
    let mut global_metadata = Vec::new();
    let mut project_files_by_index = Vec::with_capacity(config.projects().len());
    let mut run_cache = RunDocumentCache::new();
    let mut unreadable = UnreadableFiles::default();

    for (project_idx, project) in config.projects().iter().enumerate() {
        if !config.get_project_codegen_enabled(project) {
            project_files_by_index.push(Vec::new());
            continue;
        }

        let project_files = graphox_core::utils::get_project_scan_files(config, project, None);

        let import_alias = project.import().map(Arc::<str>::from);

        for path in &project_files {
            let Some(doc) = load_or_parse_document(
                path,
                documents,
                &mut run_cache,
                &mut unreadable,
                position_encoding,
            ) else {
                continue;
            };

            for frag in doc.fragments() {
                global_metadata.push(graphox_core::engine::FragmentMetadata {
                    name: frag.name.clone(),
                    path: Arc::from(path.to_string_lossy().to_string()),
                    project_idx,
                    import_alias: import_alias.clone(),
                    is_public: frag.is_public,
                    is_type_only: frag.is_type_only,
                    masked_source: Some(doc.masked_source.clone()),
                    direct_deps: frag.used_fragments.clone(),
                    transitive_deps: Arc::from([]),
                    type_fields: frag.type_fields.clone(),
                });
            }
        }

        project_files_by_index.push(project_files);
    }

    graphox_core::engine::Engine::compute_fragment_dependencies(&mut global_metadata);

    (
        global_metadata,
        project_files_by_index,
        run_cache,
        unreadable,
    )
}

/// Runs the codegen process for specified projects or all projects
#[allow(clippy::too_many_arguments)]
pub async fn run_codegen(
    client: Client,
    config: Config,
    type_caches: Arc<
        dashmap::DashMap<String, Arc<graphox_codegen::SchemaAnalysisCaches>, ahash::RandomState>,
    >,
    documents: DocumentsMap,
    supports_progress: bool,
    projects_to_run: Option<HashSet<String>>,
    position_encoding: tower_lsp::lsp_types::PositionEncodingKind,
    metadata_cache: Option<(usize, CodegenMetadataCache)>,
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

    // Reuse the cached workspace metadata (filesystem walk + fragment metadata) when
    // the workspace version is unchanged; otherwise rebuild and re-cache it. The
    // per-run document cache for closed files is always (re)built since it isn't kept
    // in the version cache.
    let cached_hit = metadata_cache.as_ref().and_then(|(version, cache)| {
        cache.read().ok().and_then(|guard| {
            guard
                .as_ref()
                .filter(|(v, _)| v == version)
                .map(|(_, m)| m.clone())
        })
    });

    let (metadata, run_cache, unreadable_files) = if let Some(metadata) = cached_hit {
        let (run_cache, unreadable) = build_run_cache(
            &metadata.project_files_by_index,
            &documents,
            &position_encoding,
        );
        (metadata, run_cache, unreadable)
    } else {
        let (global_metadata, project_files_by_index, run_cache, unreadable) =
            collect_codegen_metadata(&config, &documents, &position_encoding);
        let metadata = Arc::new(CodegenMetadata {
            global_metadata,
            project_files_by_index,
        });
        if let Some((version, cache)) = &metadata_cache
            && let Ok(mut guard) = cache.write()
        {
            *guard = Some((*version, metadata.clone()));
        }
        (metadata, run_cache, unreadable)
    };

    let global_metadata = &metadata.global_metadata;
    let project_files_by_index = &metadata.project_files_by_index;

    // Identify which projects to run
    let projects_configs: Vec<_> = config
        .projects()
        .iter()
        .enumerate()
        .filter(|p| {
            if !config.get_project_codegen_enabled(p.1) {
                return false;
            }
            if let Some(to_run) = &projects_to_run {
                to_run.contains(&p.1.include().as_key())
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

    let mut successful_projects = Vec::with_capacity(total_projects);
    // Keyed by project index, and only populated for projects that ran to completion:
    // this is both the keep-set and the safety gate for orphan pruning below.
    let mut project_outputs_by_index: ahash::AHashMap<usize, Vec<PathBuf>> =
        ahash::AHashMap::default();

    // Generate types for each project
    for (idx, (project_idx, project)) in projects_configs.iter().enumerate() {
        let current_project = idx + 1;

        let project_files = &project_files_by_index[*project_idx];

        if project_files.is_empty() {
            continue;
        }

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
                continue;
            }
        };

        let project_context = match graphox_core::engine::Engine::resolve_project_context(
            &valid_schema,
            global_metadata,
            project_files,
        ) {
            Ok(ctx) => ctx,
            Err(e) => {
                progress
                    .report(
                        &format!("Error: {}", e),
                        Some((current_project * 100 / total_projects) as u32),
                    )
                    .await;
                client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Error resolving project context for project {}: {}",
                            project.include().as_key(),
                            e
                        ),
                    )
                    .await;
                continue;
            }
        };

        // Get or create persistent type cache for this schema
        let schema_key = project.schema().as_key();
        let type_cache = type_caches
            .entry(schema_key.clone())
            .or_insert_with(|| Arc::new(graphox_codegen::SchemaAnalysisCaches::new()))
            .clone();

        let config_for_project = config.clone();
        let documents_for_project = documents.clone();
        let run_cache_for_project = run_cache.clone();
        let project_files_for_project = project_files.clone();
        let project_for_codegen = (*project).clone();
        let valid_schema = Arc::new(valid_schema);
        let project_context = Arc::new(project_context);
        let schema_import = schema_import.clone();
        let type_imports = type_imports.clone();
        let type_cache = type_cache.clone();

        let generated = match tokio::task::spawn_blocking(move || {
            let project_output_dir = project_for_codegen.output_dir().map(str::to_string);
            let codegen_config = config_for_project.get_codegen_config(Some(&project_for_codegen));

            let project_results: Vec<_> = project_files_for_project
                .par_iter()
                .filter_map(|path| {
                    let doc = get_document_for_codegen(
                        path,
                        &documents_for_project,
                        &run_cache_for_project,
                    )?;
                    if doc.get_graphql_trees().is_empty() {
                        return None;
                    }

                    let include_prefix_path = project_for_codegen
                        .include()
                        .patterns()
                        .iter()
                        .map(|pattern| graphox_core::utils::get_glob_root(pattern))
                        .find(|root| {
                            let abs_root = config_for_project.base_dir().join(root);
                            let abs_root = std::fs::canonicalize(&abs_root).unwrap_or(abs_root);
                            graphox_core::utils::path_starts_with(path, &abs_root)
                        });
                    let out_path = graphox_core::utils::get_output_path(
                        path,
                        config_for_project.base_dir(),
                        project_output_dir.as_deref().map(Path::new),
                        include_prefix_path.as_deref(),
                    );
                    let abs_out_path = if out_path.is_absolute() {
                        out_path
                    } else {
                        config_for_project.base_dir().join(out_path)
                    };
                    let codegen_path = abs_out_path.clone();

                    let ctx = graphox_codegen::CodegenContext::new(
                        valid_schema.as_ref(),
                        &project_context.fragment_to_path,
                        &project_context.fragment_to_import,
                        &project_context.fragment_to_type_only,
                        &project_context.all_fragments,
                        &project_context.name_to_id,
                        path,
                        config_for_project.scalars(),
                        &schema_import,
                        &type_imports,
                        codegen_config.generate_ast_for_fragments(),
                        &project_context.fragment_dependencies,
                        &type_cache,
                        &codegen_config,
                        {
                            if let Some(out_dir) = project_output_dir.as_deref() {
                                let out_path = graphox_core::utils::get_output_path(
                                    path,
                                    config_for_project.base_dir(),
                                    project_output_dir.as_deref().map(Path::new),
                                    include_prefix_path.as_deref(),
                                );
                                let abs_out_dir = if out_path.is_absolute() {
                                    out_path
                                        .parent()
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_else(|| out_path.clone())
                                } else {
                                    let joined = config_for_project.base_dir().join(&out_path);
                                    joined
                                        .parent()
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_else(|| joined)
                                };

                                let abs_masking_dir = config_for_project.base_dir().join(out_dir);
                                let rel_to_masking =
                                    pathdiff::diff_paths(&abs_masking_dir, &abs_out_dir)
                                        .unwrap_or_else(|| PathBuf::from("."));

                                let full_masking_path = rel_to_masking.join("fragment-masking");
                                let mut path_str =
                                    graphox_core::utils::to_posix_path(&full_masking_path);
                                if !path_str.starts_with('.')
                                    && !path_str.starts_with('/')
                                    && !full_masking_path.is_absolute()
                                {
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

                    let (ts_code, mut ops, mut frags) =
                        match graphox_codegen::generate_typescript(&doc, &ctx) {
                            Ok(generated) => generated,
                            Err(e) => {
                                // A document that still holds GraphQL but fails to
                                // generate — mid-edit, say — keeps whatever output it
                                // already has: only a source that is gone, or has no
                                // GraphQL left, is an orphan. "No executable
                                // operations" is the one failure that legitimately
                                // produces no file at all.
                                let keep = !e.contains("No executable operations");
                                return Some((
                                    keep.then_some(abs_out_path),
                                    Vec::new(),
                                    Vec::new(),
                                ));
                            }
                        };

                    let mut should_write = true;
                    if abs_out_path.exists()
                        && let Ok(existing) = std::fs::read_to_string(&abs_out_path)
                        && existing == ts_code
                    {
                        should_write = false;
                    }

                    if should_write {
                        // A failed write leaves the previous output in place, so the
                        // path still counts as claimed and must not be pruned.
                        if let Some(parent) = abs_out_path.parent()
                            && std::fs::create_dir_all(parent).is_err()
                        {
                            return Some((Some(abs_out_path), Vec::new(), Vec::new()));
                        }
                        if std::fs::write(&abs_out_path, ts_code).is_err() {
                            return Some((Some(abs_out_path), Vec::new(), Vec::new()));
                        }
                    }

                    for op in &mut ops {
                        op.codegen_path = abs_out_path.clone();
                    }
                    for frag in &mut frags {
                        frag.codegen_path = abs_out_path.clone();
                    }

                    Some((Some(abs_out_path), ops, frags))
                })
                .collect::<Vec<_>>();

            let mut project_ops = Vec::new();
            let mut project_frags: Vec<graphox_codegen::FragmentGenerated> = Vec::new();
            // Every output path this project is responsible for — the keep-set for
            // orphan pruning, which must survive a file that failed to generate.
            let mut project_output_paths: Vec<PathBuf> = Vec::new();

            for (out_path, ops, frags) in project_results {
                project_ops.extend(ops);
                project_frags.extend(frags);
                project_output_paths.extend(out_path);
            }

            (project_ops, project_frags, project_output_paths)
        })
        .await
        {
            Ok(generated) => generated,
            Err(err) => {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Codegen worker failed for project {}: {err}",
                            project.include().as_key()
                        ),
                    )
                    .await;
                continue;
            }
        };

        let (project_ops, project_frags, project_output_paths) = generated;

        // A file that exists but couldn't be read is missing from the document set,
        // which is indistinguishable from "holds no GraphQL" — so this project's
        // keep-set is incomplete and it must not be pruned against.
        let keep_set_complete = unreadable_files.is_empty()
            || !project_files
                .iter()
                .any(|file| unreadable_files.contains(file));
        if keep_set_complete {
            project_outputs_by_index.insert(*project_idx, project_output_paths);
        }
        successful_projects.push((project, project_ops, project_frags));
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

    for (project, project_ops, project_frags) in successful_projects {
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
            dir_to_config.entry(canon_out_dir_path.clone())
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

        let entrypoint_path = out_dir_path.join(format!("{}.ts", codegen_config.entrypoint_name()));
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
            codegen_config.entrypoint_name(),
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

                serde_json::json!({
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
                        let rel_path = pathdiff::diff_paths(&frag.codegen_path, &out_dir_path)
                            .unwrap_or_else(|| frag.codegen_path.clone());
                        let mut path_str = graphox_core::utils::to_posix_path(&rel_path);
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

    // Deleting a file (or emptying it of GraphQL) leaves its `.codegen.ts` behind —
    // generation only ever writes — and the orphan keeps importing symbols from the
    // outputs that were regenerated. Sweep on the same terms as the CLI, so an editor
    // session doesn't recreate the condition `graphox codegen` just fixed.
    let removed = graphox_core::utils::prune_orphaned_outputs(&config, &project_outputs_by_index);
    if !removed.is_empty() {
        client
            .log_message(
                MessageType::INFO,
                format!(
                    "Removed {} orphaned generated file(s) with no source document: {}",
                    removed.len(),
                    removed
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .await;
    }

    progress
        .end(Some(format!(
            "Generated types for {} projects",
            total_projects
        )))
        .await;
}
