use crate::config::Config;
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{get_project_files, has_generated_header, is_relevant_file};
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
    /// Direct fragment dependencies (extracted during document parsing)
    /// Contains fragment names that this fragment directly spreads
    pub direct_deps: Vec<Arc<str>>,
    /// Cached transitive fragment dependencies (computed during workspace scan)
    /// Contains all fragment names that this fragment depends on, directly or transitively
    pub transitive_deps: Vec<Arc<str>>,
    pub type_fields: Vec<(Arc<str>, Arc<str>)>,
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
    pub all_fragments: HashMap<Arc<str>, Node<executable::Fragment>>,
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
        let project_files_set: HashSet<Arc<str>> = project_files
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
        previous_metadata: Option<&WorkspaceMetadata>,
    ) -> WorkspaceMetadata {
        Self::scan_workspace_cancellable(
            config,
            |_, _| {},
            |_, _| {},
            Arc::new(AtomicBool::new(false)),
            position_encoding,
            previous_metadata,
        )
    }

    pub fn scan_workspace_cancellable<F, P>(
        config: &Config,
        mut on_doc: F,
        mut on_progress: P,
        cancelled: Arc<AtomicBool>,
        position_encoding: lsp_types::PositionEncodingKind,
        previous_metadata: Option<&WorkspaceMetadata>,
    ) -> WorkspaceMetadata
    where
        F: FnMut(PathBuf, DocumentState) + Send,
        P: FnMut(usize, usize) + Send,
    {
        let mut timings = WorkspaceScanTimings::default();

        // 1. Glob Resolution
        let start_glob = Instant::now();
        let project_info: Vec<_> = config
            .projects()
            .par_iter()
            .map(|p| {
                let abs_includes: Vec<String> = p
                    .include()
                    .patterns()
                    .iter()
                    .map(|p_inc| config.base_dir().join(p_inc).to_string_lossy().to_string())
                    .collect();
                let abs_excludes: Vec<String> = p
                    .exclude()
                    .map(|e: &crate::config::GlobPattern| e.patterns())
                    .unwrap_or_default()
                    .iter()
                    .map(|p_exc| config.base_dir().join(p_exc).to_string_lossy().to_string())
                    .collect();
                let output_dir = p.output_dir();
                (
                    get_project_files(&abs_includes, &abs_excludes, config.base_dir(), output_dir),
                    p.import().map(String::from),
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

                let full_path = config.base_dir().join(p);

                // Incremental optimization: check if file has changed
                if let Some(prev) = previous_metadata
                    && let Some(prev_doc) = prev.documents.get(p)
                {
                    let current_mtime = std::fs::metadata(&full_path)
                        .ok()
                        .and_then(|m| m.modified().ok());
                    if current_mtime == prev_doc.mtime && current_mtime.is_some() {
                        return Some((p.clone(), prev_doc.clone()));
                    }
                }

                let content = std::fs::read_to_string(&full_path).ok()?;

                if has_generated_header(&content) {
                    return None;
                }

                let abs_path = if full_path.is_absolute() {
                    full_path.clone()
                } else {
                    config
                        .base_dir()
                        .join(&full_path)
                        .canonicalize()
                        .unwrap_or_else(|_| full_path.clone())
                };
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

                let doc =
                    DocumentState::new_from_thread_local(uri, &content, position_encoding.clone());

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

        let mut prev_fragments_by_path: HashMap<Arc<str>, Vec<FragmentMetadata>> =
            HashMap::default();
        let mut prev_operations_by_path: HashMap<Arc<str>, Vec<OperationMetadata>> =
            HashMap::default();

        if let Some(prev) = previous_metadata {
            for frag in &prev.fragments {
                prev_fragments_by_path
                    .entry(frag.path.clone())
                    .or_default()
                    .push(frag.clone());
            }
            for op in &prev.operations {
                prev_operations_by_path
                    .entry(op.path.clone())
                    .or_default()
                    .push(op.clone());
            }
        }

        let path_to_doc_ref = &path_to_doc;
        let prev_metadata_ref = previous_metadata;
        let prev_frags_ref = &prev_fragments_by_path;
        let prev_ops_ref = &prev_operations_by_path;

        let (mut all_fragments, all_operations, reused_fragments_count): (Vec<_>, Vec<_>, usize) =
            project_info
                .par_iter()
                .map(|(paths, import_alias)| {
                    let import_alias_arc = import_alias.as_deref().map(Arc::from);
                    let mut fragments = Vec::new();
                    let mut operations = Vec::new();
                    let mut reused_count = 0;

                    for path in paths {
                        if let Some(doc) = path_to_doc_ref.get(path) {
                            let path_str: Arc<str> = path.to_string_lossy().to_string().into();

                            let mut reused = false;
                            if let Some(prev) = prev_metadata_ref
                                && let Some(prev_doc) = prev.documents.get(path)
                                && prev_doc.mtime == doc.mtime
                                && prev_doc.mtime.is_some()
                                && prev_frags_ref.get(&path_str).is_none_or(|frags| {
                                    frags.iter().all(|f| f.import_alias == import_alias_arc)
                                })
                            {
                                if let Some(prev_frags) = prev_frags_ref.get(&path_str) {
                                    reused_count += prev_frags.len();
                                    fragments.extend(prev_frags.iter().cloned());
                                }
                                if let Some(prev_ops) = prev_ops_ref.get(&path_str) {
                                    operations.extend(prev_ops.iter().cloned());
                                }
                                reused = true;
                            }

                            if !reused {
                                for frag in doc.fragments() {
                                    fragments.push(FragmentMetadata {
                                        name: frag.name.clone(),
                                        path: path_str.clone(),
                                        import_alias: import_alias_arc.clone(),
                                        is_public: frag.is_public,
                                        is_type_only: frag.is_type_only,
                                        masked_source: doc.masked_source.clone(),
                                        direct_deps: frag.used_fragments.clone(),
                                        transitive_deps: Vec::new(),
                                        type_fields: frag.type_fields.clone(),
                                    });
                                }

                                for op in doc.operations() {
                                    operations.push(OperationMetadata {
                                        name: op.name.clone(),
                                        path: path_str.clone(),
                                        source_text: op.source_text.clone(),
                                        operation_type: op.operation_type.clone(),
                                    });
                                }
                            }
                        }
                    }
                    (fragments, operations, reused_count)
                })
                .reduce(
                    || (Vec::new(), Vec::new(), 0),
                    |mut acc, (f, o, r)| {
                        acc.0.extend(f);
                        acc.1.extend(o);
                        acc.2 += r;
                        acc
                    },
                );

        timings.metadata_extraction = start_metadata.elapsed();

        // 5. Compute transitive fragment dependencies
        // This is done once during workspace scan to avoid repeated computation during codegen
        let start_deps = Instant::now();
        if let Some(prev) = previous_metadata
            && reused_fragments_count == all_fragments.len()
            && all_fragments.len() == prev.fragments.len()
        {
            // Optimization: All fragments were reused and no fragments were added or removed.
            // Transitive dependencies are already populated and correct.
        } else {
            Self::compute_fragment_dependencies(&mut all_fragments);
        }
        timings.fragment_deps_computation = start_deps.elapsed();

        // 6. Build operation name index for duplicate detection
        let operation_names_by_project = project_info
            .par_iter()
            .enumerate()
            .map(|(project_idx, (paths, _))| {
                let mut local_map: HashMap<Arc<str>, HashMap<usize, Vec<PathBuf>>> =
                    HashMap::default();
                for path in paths {
                    if let Some(doc) = path_to_doc_ref.get(path) {
                        for op in doc.operations() {
                            if let Some(name) = &op.name {
                                local_map
                                    .entry(name.clone())
                                    .or_default()
                                    .entry(project_idx)
                                    .or_default()
                                    .push(path.clone());
                            }
                        }
                    }
                }
                local_map
            })
            .reduce(HashMap::default, |mut acc, local_map| {
                for (name, project_map) in local_map {
                    let entry = acc.entry(name).or_default();
                    for (project_idx, paths) in project_map {
                        entry.entry(project_idx).or_default().extend(paths);
                    }
                }
                acc
            });

        WorkspaceMetadata {
            fragments: all_fragments,
            operations: all_operations,
            projects: project_info
                .into_iter()
                .zip(config.projects())
                .map(|((files, _), p)| ProjectMetadata {
                    include_key: p.include().as_key(),
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
    ) -> HashMap<Arc<str>, apollo_compiler::Node<executable::Fragment>> {
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

        // Pre-allocate HashMap with known capacity - use Arc<str> to avoid string allocations
        let mut all_fragments: HashMap<Arc<str>, Node<executable::Fragment>> =
            HashMap::with_capacity_and_hasher(exec_doc.fragments.len(), Default::default());
        for (name, frag) in exec_doc.fragments {
            all_fragments.insert(Arc::from(name.as_str()), frag);
        }
        all_fragments
    }

    /// Compute transitive fragment dependencies for all fragments
    /// This is called once during workspace scan to cache dependencies
    fn compute_fragment_dependencies(fragments: &mut [FragmentMetadata]) {
        let n = fragments.len();
        if n == 0 {
            return;
        }

        // 1. Build a map of fragment name -> index for O(1) integer-based lookup
        let mut name_to_idx = HashMap::with_capacity(n);
        for (i, f) in fragments.iter().enumerate() {
            name_to_idx.insert(f.name.clone(), i);
        }

        // 2. Use pre-extracted direct dependencies from document parsing
        // This avoids redundant regex matching since used_fragments was already extracted
        // during extract_symbols() via Tree-sitter queries
        let direct_deps_idx: Vec<Vec<usize>> = fragments
            .par_iter()
            .map(|frag| {
                let mut deps = Vec::new();
                for dep_name in &frag.direct_deps {
                    if let Some(&idx) = name_to_idx.get(dep_name)
                        && idx != name_to_idx[&frag.name]
                    {
                        deps.push(idx);
                    }
                }
                deps.sort_unstable();
                deps.dedup();
                deps
            })
            .collect();

        // 3. Find SCCs using iterative Tarjan's (Sequential O(V+E))
        let mut index = 0;
        let mut indices = vec![-1; n];
        let mut lowlink = vec![-1; n];
        let mut on_stack = vec![false; n];
        let mut stack = Vec::new();
        let mut sccs = Vec::new();
        let mut call_stack = Vec::new();

        for i in 0..n {
            if indices[i] == -1 {
                call_stack.push((i, 0)); // (node, next_child_idx)
                while let Some((v, child_idx)) = call_stack.last_mut() {
                    let v = *v;
                    if *child_idx == 0 {
                        indices[v] = index;
                        lowlink[v] = index;
                        index += 1;
                        stack.push(v);
                        on_stack[v] = true;
                    }

                    let mut found_child = false;
                    let children = &direct_deps_idx[v];
                    for (i, &w) in children.iter().enumerate().skip(*child_idx) {
                        *child_idx = i + 1;
                        if indices[w] == -1 {
                            call_stack.push((w, 0));
                            found_child = true;
                            break;
                        } else if on_stack[w] {
                            lowlink[v] = lowlink[v].min(indices[w]);
                        }
                    }

                    if found_child {
                        continue;
                    }

                    // Finished visiting v
                    if lowlink[v] == indices[v] {
                        let mut scc = Vec::new();
                        while let Some(w) = stack.pop() {
                            on_stack[w] = false;
                            scc.push(w);
                            if w == v {
                                break;
                            }
                        }
                        sccs.push(scc);
                    }

                    call_stack.pop();
                    if let Some((parent, _)) = call_stack.last_mut() {
                        lowlink[*parent] = lowlink[*parent].min(lowlink[v]);
                    }
                }
            }
        }

        // 4. Compute transitive closure on the condensation graph using bitsets
        let words = n.div_ceil(64);
        let mut scc_bitsets = vec![0u64; sccs.len() * words];
        let mut node_to_scc = vec![0; n];
        for (scc_idx, scc) in sccs.iter().enumerate() {
            for &node in scc {
                node_to_scc[node] = scc_idx;
                // Each node in SCC depends on all other nodes in the same SCC
                for &other in scc {
                    if node != other {
                        scc_bitsets[scc_idx * words + (other / 64)] |= 1 << (other % 64);
                    }
                }
                // And its direct external dependencies
                for &dep in &direct_deps_idx[node] {
                    scc_bitsets[scc_idx * words + (dep / 64)] |= 1 << (dep % 64);
                }
            }
        }

        // Propagate bitsets in reverse topological order (Tarjan's produces it)
        for (scc_idx, scc) in sccs.iter().enumerate() {
            let mut external_scc_deps = HashSet::default();
            for &node in scc {
                for &dep in &direct_deps_idx[node] {
                    let dep_scc_idx = node_to_scc[dep];
                    if dep_scc_idx != scc_idx {
                        external_scc_deps.insert(dep_scc_idx);
                    }
                }
            }

            for dep_scc_idx in external_scc_deps {
                let (earlier_sccs, later_sccs) = scc_bitsets.split_at_mut(scc_idx * words);
                let current_bs = &mut later_sccs[0..words];
                let dep_bs = &earlier_sccs[dep_scc_idx * words..dep_scc_idx * words + words];
                for w in 0..words {
                    current_bs[w] |= dep_bs[w];
                }
            }
        }

        // 5. Map bitsets back to fragments and convert to names (Parallel)
        let transitive_results: Vec<Vec<Arc<str>>> = (0..n)
            .into_par_iter()
            .map(|i| {
                let scc_idx = node_to_scc[i];
                let bitset = &scc_bitsets[scc_idx * words..(scc_idx + 1) * words];
                let mut res = Vec::new();
                for (word_idx, mut word) in bitset.iter().copied().enumerate() {
                    while word != 0 {
                        let bit_idx = word.trailing_zeros();
                        let idx = word_idx * 64 + bit_idx as usize;
                        if idx < n {
                            res.push(fragments[idx].name.clone());
                        }
                        word &= !(1 << bit_idx);
                    }
                }
                res.sort_unstable();
                res
            })
            .collect();

        for (i, res) in transitive_results.into_iter().enumerate() {
            fragments[i].transitive_deps = res;
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

        let doc = DocumentState::new_from_thread_local(uri, &content, position_encoding);

        Some(doc)
    }
}
