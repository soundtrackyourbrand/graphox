use crate::config::Config;
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{get_project_files, is_relevant_file};
use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::{Node, Schema, executable};
use lsp_types::Url;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct FragmentMetadata {
    pub name: Arc<str>,
    pub path: Arc<str>,
    pub import_alias: Option<Arc<str>>,
    pub is_public: bool,
    pub is_type_only: bool,
    pub masked_source: Arc<str>,
    /// Cached transitive fragment dependencies (computed during workspace scan)
    /// Contains all fragment names that this fragment depends on, directly or transitively
    pub transitive_deps: Vec<Arc<str>>,
}

#[derive(Debug, Clone)]
pub struct OperationMetadata {
    pub name: Option<Arc<str>>,
    pub path: Arc<str>,
    pub source_text: Arc<str>,
    pub operation_type: Arc<str>,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceScanTimings {
    pub glob_resolution: Duration,
    pub doc_parsing: Duration,
    pub metadata_extraction: Duration,
    pub fragment_deps_computation: Duration,
}

#[derive(Debug, Clone)]
pub struct ProjectMetadata {
    pub include_key: String,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceMetadata {
    pub fragments: Vec<FragmentMetadata>,
    pub operations: Vec<OperationMetadata>,
    pub projects: Vec<ProjectMetadata>,
    pub timings: WorkspaceScanTimings,
    pub documents: HashMap<PathBuf, DocumentState>,
    /// Maps operation name -> project index -> list of file paths
    /// Used to detect duplicate operation names within a project
    pub operation_names_by_project: HashMap<Arc<str>, HashMap<usize, Vec<PathBuf>>>,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub fragment_to_path: HashMap<Arc<str>, Arc<str>>,
    pub fragment_to_import: HashMap<Arc<str>, Arc<str>>,
    pub fragment_to_type_only: HashMap<Arc<str>, bool>,
    pub all_fragments: HashMap<String, Node<executable::Fragment>>,
    /// Cached fragment dependencies: fragment name -> list of transitive dependencies
    pub fragment_dependencies: HashMap<Arc<str>, Vec<Arc<str>>>,
}

pub struct Engine;

impl Engine {
    pub fn resolve_project_context(
        valid_schema: &apollo_compiler::validation::Valid<Schema>,
        global_metadata: &[FragmentMetadata],
        project_files: &[PathBuf],
    ) -> ProjectContext {
        let project_files_set: std::collections::HashSet<Arc<str>> = project_files
            .iter()
            .map(|p| Arc::from(p.to_string_lossy().to_string()))
            .collect();

        let mut fragment_to_path: HashMap<Arc<str>, Arc<str>> = HashMap::default();
        let mut fragment_to_import: HashMap<Arc<str>, Arc<str>> = HashMap::default();
        let mut fragment_to_type_only: HashMap<Arc<str>, bool> = HashMap::default();
        let mut project_fragments_metadata = Vec::new();

        for meta in global_metadata {
            // Avoid expensive canonicalize calls by checking the path as-is first
            let is_local = project_files_set.contains(&meta.path);
            if is_local {
                fragment_to_path.insert(meta.name.clone(), meta.path.clone());
                if let Some(a) = &meta.import_alias {
                    fragment_to_import.insert(meta.name.clone(), a.clone());
                }
                fragment_to_type_only.insert(meta.name.clone(), meta.is_type_only);
                project_fragments_metadata.push(meta.clone());
            } else if meta.is_public {
                let existing_local = fragment_to_path.contains_key(&meta.name)
                    && project_files_set.contains(fragment_to_path.get(&meta.name).unwrap());

                if !existing_local {
                    fragment_to_path
                        .entry(meta.name.clone())
                        .or_insert_with(|| meta.path.clone());
                    if let Some(a) = &meta.import_alias {
                        fragment_to_import
                            .entry(meta.name.clone())
                            .or_insert_with(|| a.clone());
                    }
                    fragment_to_type_only.insert(meta.name.clone(), meta.is_type_only);
                    project_fragments_metadata.push(meta.clone());
                }
            }
        }

        let all_fragments = Self::resolve_fragments(valid_schema, &project_fragments_metadata);

        // Build fragment dependency cache from the metadata
        let mut fragment_dependencies: HashMap<Arc<str>, Vec<Arc<str>>> = HashMap::default();
        for meta in &project_fragments_metadata {
            fragment_dependencies.insert(meta.name.clone(), meta.transitive_deps.clone());
        }

        ProjectContext {
            fragment_to_path,
            fragment_to_import,
            fragment_to_type_only,
            all_fragments,
            fragment_dependencies,
        }
    }

    pub fn scan_workspace(
        config: &Config,
        position_encoding: lsp_types::PositionEncodingKind,
    ) -> WorkspaceMetadata {
        Self::scan_workspace_cancellable(
            config,
            |_, _| {},
            |_, _| {},
            Arc::new(AtomicBool::new(false)),
            position_encoding,
        )
    }

    pub fn scan_workspace_cancellable<F, P>(
        config: &Config,
        mut on_doc: F,
        mut on_progress: P,
        cancelled: Arc<AtomicBool>,
        position_encoding: lsp_types::PositionEncodingKind,
    ) -> WorkspaceMetadata
    where
        F: FnMut(PathBuf, DocumentState) + Send,
        P: FnMut(usize, usize) + Send,
    {
        let mut timings = WorkspaceScanTimings::default();

        // 1. Glob Resolution
        let start_glob = Instant::now();
        let project_info: Vec<_> = config
            .projects
            .par_iter()
            .map(|p| {
                let abs_includes: Vec<String> = p
                    .include
                    .patterns()
                    .iter()
                    .map(|p_inc| config.base_dir.join(p_inc).to_string_lossy().to_string())
                    .collect();
                let abs_excludes: Vec<String> = p
                    .exclude
                    .as_ref()
                    .map(|e| e.patterns())
                    .unwrap_or_default()
                    .iter()
                    .map(|p_exc| config.base_dir.join(p_exc).to_string_lossy().to_string())
                    .collect();
                (
                    get_project_files(&abs_includes, &abs_excludes, &config.base_dir),
                    p.import.clone(),
                )
            })
            .collect();
        timings.glob_resolution = start_glob.elapsed();

        // 2. Unique File Identification
        let mut all_unique_paths = HashSet::default();
        for (paths, _) in &project_info {
            for path in paths {
                all_unique_paths.insert(path.clone());
            }
        }

        // 3. Parallel Document Parsing
        let start_parse = Instant::now();
        let docs_vec: Vec<(PathBuf, DocumentState)> = all_unique_paths
            .par_iter()
            .filter(|p| is_relevant_file(p))
            .filter_map(|p| {
                if cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                let content = std::fs::read_to_string(p).ok()?;
                let abs_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
                let uri = Url::from_file_path(&abs_path).ok()?;
                let language = DocumentLanguage::from_uri(&uri);

                if language.is_host_language() {
                    let bytes = content.as_bytes();
                    let has_gql = bytes.windows(3).any(|w| w.eq_ignore_ascii_case(b"gql"))
                        || bytes.windows(7).any(|w| w.eq_ignore_ascii_case(b"graphql"));
                    if !has_gql {
                        return None;
                    }
                }

                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&language.get_parser_language()).ok()?;
                let doc = DocumentState::new(uri, &content, parser, position_encoding.clone());

                Some((p.clone(), doc))
            })
            .collect();

        let mut path_to_doc = HashMap::default();
        let total_docs = docs_vec.len();
        for (i, (p, doc)) in docs_vec.into_iter().enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            on_progress(i, total_docs);

            // Helpful for debugging large workspaces
            if i % 100 == 0 || i == total_docs - 1 {
                // println!("Parsing file {}/{}", i + 1, total_docs);
            }

            on_doc(p.clone(), doc.clone());
            path_to_doc.insert(p, doc);
        }

        timings.doc_parsing = start_parse.elapsed();

        // 4. Metadata Extraction & Project Association
        let start_metadata = Instant::now();
        let mut all_fragments = Vec::new();
        let mut all_operations = Vec::new();

        for (paths, import_alias) in &project_info {
            let import_alias_arc = import_alias.as_deref().map(Arc::from);
            for path in paths {
                if let Some(doc) = path_to_doc.get(path) {
                    let path_str: Arc<str> = path.to_string_lossy().to_string().into();

                    for frag in doc.fragments() {
                        all_fragments.push(FragmentMetadata {
                            name: frag.name.clone(),
                            path: path_str.clone(),
                            import_alias: import_alias_arc.clone(),
                            is_public: frag.is_public,
                            is_type_only: frag.is_type_only,
                            masked_source: doc.masked_source.clone(),
                            transitive_deps: Vec::new(), // Will be populated after all fragments are collected
                        });
                    }

                    for op in doc.operations() {
                        all_operations.push(OperationMetadata {
                            name: op.name.clone(),
                            path: path_str.clone(),
                            source_text: op.source_text.clone(),
                            operation_type: op.operation_type.clone(),
                        });
                    }
                }
            }
        }
        timings.metadata_extraction = start_metadata.elapsed();

        // 5. Compute transitive fragment dependencies
        // This is done once during workspace scan to avoid repeated computation during codegen
        let start_deps = Instant::now();
        Self::compute_fragment_dependencies(&mut all_fragments);
        timings.fragment_deps_computation = start_deps.elapsed();

        // 6. Build operation name index for duplicate detection
        let mut operation_names_by_project: HashMap<Arc<str>, HashMap<usize, Vec<PathBuf>>> =
            HashMap::default();
        for (project_idx, (paths, _)) in project_info.iter().enumerate() {
            for path in paths {
                if let Some(doc) = path_to_doc.get(path) {
                    for op in doc.operations() {
                        if let Some(name) = &op.name {
                            operation_names_by_project
                                .entry(name.clone())
                                .or_default()
                                .entry(project_idx)
                                .or_default()
                                .push(path.clone());
                        }
                    }
                }
            }
        }

        WorkspaceMetadata {
            fragments: all_fragments,
            operations: all_operations,
            projects: project_info
                .into_iter()
                .zip(&config.projects)
                .map(|((files, _), p)| ProjectMetadata {
                    include_key: p.include.as_key(),
                    files,
                })
                .collect(),
            timings,
            documents: path_to_doc,
            operation_names_by_project,
        }
    }

    /// Transitive Fragment Resolving for a specific schema
    pub fn resolve_fragments(
        valid_schema: &apollo_compiler::validation::Valid<Schema>,
        fragments: &[FragmentMetadata],
    ) -> HashMap<String, apollo_compiler::Node<executable::Fragment>> {
        // Pre-allocate with estimated capacity to reduce reallocations
        let estimated_size: usize = fragments.iter().map(|f| f.masked_source.len() + 1).sum();
        let mut combined_source = String::with_capacity(estimated_size);
        let mut seen_paths = HashSet::default();

        for frag in fragments {
            if seen_paths.insert(&frag.path) {
                combined_source.push_str(&frag.masked_source);
                combined_source.push('\n');
            }
        }

        // ExecutableDocument::parse will resolve all fragment spreads against each other.
        let exec_doc = match executable::ExecutableDocument::parse(
            valid_schema,
            combined_source,
            "workspace.graphql",
        ) {
            Ok(doc) => doc,
            Err(with_errors) => with_errors.partial,
        };

        // Pre-allocate HashMap with known capacity
        let mut all_fragments =
            HashMap::with_capacity_and_hasher(exec_doc.fragments.len(), Default::default());
        for (name, frag) in exec_doc.fragments {
            all_fragments.insert(name.as_str().to_string(), frag.clone());
        }
        all_fragments
    }

    /// Compute transitive fragment dependencies for all fragments
    /// This is called once during workspace scan to cache dependencies
    fn compute_fragment_dependencies(fragments: &mut [FragmentMetadata]) {
        // Build a map of fragment name -> direct dependencies
        // We use a simple pattern matching approach that's faster than full parsing
        let mut direct_deps: HashMap<Arc<str>, Vec<Arc<str>>> = HashMap::default();

        // Build fragment name set for quick lookup
        let fragment_names: HashSet<Arc<str>> = fragments.iter().map(|f| f.name.clone()).collect();

        // Parallelize the direct dependency computation
        let deps_vec: Vec<(Arc<str>, Vec<Arc<str>>)> = fragments
            .par_iter()
            .map(|frag| {
                let mut deps = Vec::new();
                let source = &frag.masked_source;

                // Quick heuristic: only look for fragments if source contains "..."
                if !source.contains("...") {
                    return (frag.name.clone(), deps);
                }

                // Look for fragment spreads: ...FragmentName
                // More efficient pattern matching without repeated allocations
                for other_frag_name in &fragment_names {
                    if frag.name == *other_frag_name {
                        continue;
                    }

                    // Build pattern once and search for it
                    let spread_marker = "...";
                    let mut search_offset = 0;

                    // Search for all occurrences of the fragment name after "..."
                    while let Some(marker_pos) = source[search_offset..].find(spread_marker) {
                        let actual_pos = search_offset + marker_pos;
                        let name_start = actual_pos + spread_marker.len();

                        // Check if fragment name follows
                        if name_start + other_frag_name.len() <= source.len() {
                            let potential_name =
                                &source[name_start..name_start + other_frag_name.len()];

                            if potential_name == other_frag_name.as_ref() {
                                // Verify it's not part of a longer identifier
                                let end_idx = name_start + other_frag_name.len();
                                let is_valid = if end_idx < source.len() {
                                    let next_char = source[end_idx..].chars().next().unwrap();
                                    !next_char.is_alphanumeric() && next_char != '_'
                                } else {
                                    true // End of string is valid
                                };

                                if is_valid {
                                    deps.push(other_frag_name.clone());
                                    break; // Found this fragment, no need to keep searching
                                }
                            }
                        }

                        search_offset = actual_pos + 1;
                    }
                }

                (frag.name.clone(), deps)
            })
            .collect();

        // Convert to HashMap
        for (name, deps) in deps_vec {
            direct_deps.insert(name, deps);
        }

        // Compute transitive closure using DFS with memoization
        fn get_transitive_deps(
            frag_name: &Arc<str>,
            direct_deps: &HashMap<Arc<str>, Vec<Arc<str>>>,
            memo: &mut HashMap<Arc<str>, Vec<Arc<str>>>,
            visited: &mut HashSet<Arc<str>>,
        ) -> Vec<Arc<str>> {
            // Check memo first
            if let Some(cached) = memo.get(frag_name) {
                return cached.clone();
            }

            // Cycle detection
            if visited.contains(frag_name) {
                return Vec::new();
            }
            visited.insert(frag_name.clone());

            let mut all_deps = HashSet::default();
            if let Some(deps) = direct_deps.get(frag_name) {
                for dep in deps {
                    all_deps.insert(dep.clone());
                    // Recursively get transitive deps
                    for transitive_dep in get_transitive_deps(dep, direct_deps, memo, visited) {
                        all_deps.insert(transitive_dep);
                    }
                }
            }

            visited.remove(frag_name);
            let mut result: Vec<_> = all_deps.into_iter().collect();
            result.sort(); // Keep consistent ordering

            // Cache the result
            memo.insert(frag_name.clone(), result.clone());
            result
        }

        // Populate the transitive_deps field for each fragment with memoization
        let mut memo: HashMap<Arc<str>, Vec<Arc<str>>> = HashMap::default();
        for frag in fragments.iter_mut() {
            let mut visited = HashSet::default();
            frag.transitive_deps =
                get_transitive_deps(&frag.name, &direct_deps, &mut memo, &mut visited);
        }
    }

    pub fn parse_doc(
        path: &Path,
        position_encoding: lsp_types::PositionEncodingKind,
    ) -> Option<DocumentState> {
        let content = std::fs::read_to_string(path).ok()?;
        let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let uri = Url::from_file_path(&abs_path).ok()?;
        let language = DocumentLanguage::from_uri(&uri);

        if language.is_host_language() {
            let bytes = content.as_bytes();
            let has_gql = bytes.windows(3).any(|w| w.eq_ignore_ascii_case(b"gql"))
                || bytes.windows(7).any(|w| w.eq_ignore_ascii_case(b"graphql"));
            if !has_gql {
                return None;
            }
        }

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language.get_parser_language()).ok()?;
        let doc = DocumentState::new(uri, &content, parser, position_encoding);

        Some(doc)
    }
}
