use crate::config::{Config, SchemaSource};
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{get_project_files, is_relevant_file, mask_interpolations};
use apollo_compiler::{executable, Schema};
use fnv::FnvHashMap as HashMap;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

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

#[derive(Debug, Clone)]
pub struct WorkspaceMetadata {
    pub fragments: Vec<FragmentMetadata>,
    pub operations: Vec<OperationMetadata>,
}

pub struct Engine;

impl Engine {
    /// Step 1: Discover all fragments and operations across the entire workspace
    pub fn scan_workspace(config: &Config) -> WorkspaceMetadata {
        let projects: Vec<_> = config
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
                (abs_includes, abs_excludes, p.import.clone())
            })
            .collect();

        let scan_results: Vec<(Vec<FragmentMetadata>, Vec<OperationMetadata>)> = projects
            .par_iter()
            .map(|(abs_includes, abs_excludes, import_alias)| {
                let paths = get_project_files(abs_includes, abs_excludes);
                let mut fragments = Vec::new();
                let mut operations = Vec::new();
                for path in paths {
                    if is_relevant_file(&path)
                        && let Some(doc) = Self::parse_doc(&path)
                    {
                        for frag in doc.fragments() {
                            fragments.push(FragmentMetadata {
                                name: frag.name.clone(),
                                path: path.to_string_lossy().to_string(),
                                import_alias: import_alias.clone(),
                                is_public: frag.is_public,
                            });
                        }
                        for op in doc.operations() {
                            operations.push(OperationMetadata {
                                name: op.name.clone(),
                                path: path.to_string_lossy().to_string(),
                                source_text: op.source_text.clone(),
                                operation_type: op.operation_type.clone(),
                            });
                        }
                    }
                }
                (fragments, operations)
            })
            .collect();

        let mut all_fragments = Vec::new();
        let mut all_operations = Vec::new();

        for (fragments, operations) in scan_results {
            all_fragments.extend(fragments);
            all_operations.extend(operations);
        }

        WorkspaceMetadata {
            fragments: all_fragments,
            operations: all_operations,
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
                Err(e) => return Err(format!("Failed to read schema file {}: {}", path.display(), e)),
            }
        }
        Schema::parse(&combined_text, source.as_key())
            .map_err(|e| format!("Failed to parse schema {}: {}", source.as_key(), e))
    }
}
