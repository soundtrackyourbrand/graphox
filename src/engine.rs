use crate::config::{Config, SchemaSource};
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{get_project_files, is_relevant_file};
use apollo_compiler::{executable, Node, Schema};
use fnv::FnvHashMap as HashMap;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_lsp::lsp_types::Url;

#[derive(Debug, Clone)]
pub struct FragmentMetadata {
    pub name: String,
    pub path: String,
    pub import_alias: Option<String>,
    pub is_public: bool,
    pub masked_source: String,
}

#[derive(Debug, Clone)]
pub struct OperationMetadata {
    pub name: Option<String>,
    pub path: String,
    pub source_text: String,
    pub operation_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceScanTimings {
    pub glob_resolution: Duration,
    pub doc_parsing: Duration,
    pub metadata_extraction: Duration,
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
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub fragment_to_path: HashMap<String, String>,
    pub fragment_to_import: HashMap<String, String>,
    pub all_fragments: HashMap<String, Node<executable::Fragment>>,
}

pub struct Engine;

impl Engine {
    pub fn resolve_project_context(
        valid_schema: &apollo_compiler::validation::Valid<Schema>,
        global_metadata: &[FragmentMetadata],
        project_files: &[PathBuf],
    ) -> ProjectContext {
        let project_files_set: fnv::FnvHashSet<String> = project_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

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

        let all_fragments = Self::resolve_fragments(valid_schema, global_metadata);

        ProjectContext {
            fragment_to_path,
            fragment_to_import,
            all_fragments,
        }
    }
    /// Step 1: Discover all fragments and operations across the entire workspace
    pub fn scan_workspace<F>(config: &Config, on_doc: F) -> WorkspaceMetadata
    where
        F: FnMut(PathBuf, DocumentState) + Send,
    {
        Self::scan_workspace_cancellable(config, on_doc, Arc::new(AtomicBool::new(false)))
    }

    pub fn scan_workspace_cancellable<F>(
        config: &Config,
        mut on_doc: F,
        cancelled: Arc<AtomicBool>,
    ) -> WorkspaceMetadata
    where
        F: FnMut(PathBuf, DocumentState) + Send,
    {
        let mut timings = WorkspaceScanTimings::default();

        // 1. Glob Resolution
        let start_glob = Instant::now();
        let project_info: Vec<_> = config
            .projects
            .iter()
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
                    get_project_files(&abs_includes, &abs_excludes),
                    p.import.clone(),
                )
            })
            .collect();
        timings.glob_resolution = start_glob.elapsed();

        // 2. Unique File Identification
        let mut all_unique_paths = fnv::FnvHashSet::default();
        for (paths, _) in &project_info {
            for path in paths {
                all_unique_paths.insert(path.clone());
            }
        }

        // 3. Parallel Document Parsing
        let start_parse = Instant::now();
        let docs_vec: Vec<(PathBuf, DocumentState)> = all_unique_paths
            .into_par_iter()
            .filter(|p| is_relevant_file(p))
            .filter_map(|p| {
                if cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                let content = std::fs::read_to_string(&p).ok()?;
                let abs_path = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
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
                let doc = DocumentState::new(uri, &content, parser);

                Some((p, doc))
            })
            .collect();

        let mut path_to_doc = HashMap::default();
        for (p, doc) in docs_vec {
            if cancelled.load(Ordering::Relaxed) {
                break;
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
            for path in paths {
                if let Some(doc) = path_to_doc.get(path) {
                    let path_str = path.to_string_lossy().to_string();

                    for frag in doc.fragments() {
                        // eprintln!("DEBUG: Found fragment {} in {}", frag.name, path_str);
                        all_fragments.push(FragmentMetadata {
                            name: frag.name.clone(),
                            path: path_str.clone(),
                            import_alias: import_alias.clone(),
                            is_public: frag.is_public,
                            masked_source: doc.masked_source.clone(),
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
        }
    }

    /// Step 1b: Discovery for simple mode (no config)
    pub fn scan_path(scan_path: &str) -> HashMap<String, String> {
        let mut fragment_map = HashMap::default();
        let paths = get_project_files(&[scan_path.to_string()], &[]);

        let results: Vec<Vec<_>> = paths
            .par_iter()
            .filter(|p| is_relevant_file(p))
            .map(|path| {
                let mut frags = Vec::new();
                if let Some(doc) = Self::parse_doc(path) {
                    for frag in doc.fragments() {
                        frags.push((frag.name.clone(), path.to_string_lossy().to_string()));
                    }
                }
                frags
            })
            .collect();

        for frags in results {
            for (name, path) in frags {
                fragment_map.insert(name, path);
            }
        }
        fragment_map
    }

    /// Step 2: Transitive Fragment Resolving for a specific schema
    pub fn resolve_fragments(
        valid_schema: &apollo_compiler::validation::Valid<Schema>,
        fragments: &[FragmentMetadata],
    ) -> HashMap<String, apollo_compiler::Node<executable::Fragment>> {
        let mut combined_source = String::new();
        let mut seen_paths = fnv::FnvHashSet::default();
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

        let mut all_fragments = HashMap::default();
        for (name, frag) in exec_doc.fragments {
            all_fragments.insert(name.as_str().to_string(), frag.clone());
        }
        all_fragments
    }

    pub fn parse_doc(path: &Path) -> Option<DocumentState> {
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
        let doc = DocumentState::new(uri, &content, parser);

        Some(doc)
    }

    pub fn load_schema(base_dir: &Path, source: &SchemaSource) -> Result<Schema, String> {
        let mut texts = Vec::new();
        for file in source.files() {
            let path = base_dir.join(file);
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    texts.push(text);
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to read schema file {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        }
        let combined_text = crate::utils::merge_schema_texts(&texts);
        Schema::parse(&combined_text, source.as_key())
            .map_err(|e| format!("Failed to parse schema {}: {}", source.as_key(), e))
    }
}
