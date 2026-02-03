use crate::config::{Config, SchemaSource};
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{get_project_files, is_relevant_file, mask_interpolations};
use apollo_compiler::{executable, Schema};
use fnv::FnvHashMap as HashMap;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct FragmentMetadata {
    pub name: String,
    pub path: String,
    pub import_alias: Option<String>,
    pub is_public: bool,
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
}

pub struct Engine;

impl Engine {
    /// Step 1: Discover all fragments and operations across the entire workspace
    pub fn scan_workspace(config: &Config) -> WorkspaceMetadata {
        let mut timings = WorkspaceScanTimings::default();

        // 1. Glob Resolution: Find which files belong to which projects
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
        let path_to_doc: HashMap<PathBuf, DocumentState> = all_unique_paths
            .into_par_iter()
            .filter(|p| is_relevant_file(p))
            .filter_map(|p| Self::parse_doc(&p).map(|doc| (p, doc)))
            .collect();
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
                        all_fragments.push(FragmentMetadata {
                            name: frag.name.clone(),
                            path: path_str.clone(),
                            import_alias: import_alias.clone(),
                            is_public: frag.is_public,
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
        all_graphql_paths: &[PathBuf],
    ) -> HashMap<String, apollo_compiler::Node<executable::Fragment>> {
        let mut all_fragments = HashMap::default();

        let fragment_results: Vec<Vec<_>> = all_graphql_paths
            .par_iter()
            .map(|path| {
                let mut frags = Vec::new();
                if let Some(doc) = Self::parse_doc(path) {
                    for block in doc.get_graphql_trees() {
                        let block_text = doc.get_node_text(block.tree.root_node(), block.offset);
                        let masked = mask_interpolations(&block_text);
                        if let Ok(exec_doc) = executable::ExecutableDocument::parse(
                            valid_schema,
                            &masked,
                            "doc.graphql",
                        ) {
                            for (name, frag) in exec_doc.fragments {
                                frags.push((name.to_string(), frag.clone()));
                            }
                        }
                    }
                }
                frags
            })
            .collect();

        for frags in fragment_results {
            for (name, frag) in frags {
                all_fragments.insert(name, frag);
            }
        }
        all_fragments
    }

    pub fn parse_doc(path: &Path) -> Option<DocumentState> {
        let content = std::fs::read_to_string(path).ok()?;
        let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let uri = Url::from_file_path(&abs_path).ok()?;
        let language = DocumentLanguage::from_uri(&uri);

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language.get_parser_language()).ok()?;
        let doc = DocumentState::new(uri, &content, parser);

        if language.is_host_language() && !doc.has_graphql_candidates() {
            return None;
        }

        Some(doc)
    }

    pub fn load_schema(base_dir: &Path, source: &SchemaSource) -> Result<Schema, String> {
        let mut combined_text = String::new();
        for file in source.files() {
            let path = base_dir.join(file);
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    combined_text.push_str(&text);
                    combined_text.push('\n');
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to read schema file {}: {}",
                        path.display(),
                        e
                    ))
                }
            }
        }
        Schema::parse(&combined_text, source.as_key())
            .map_err(|e| format!("Failed to parse schema {}: {}", source.as_key(), e))
    }
}
