use crate::config::{Config, SchemaSource};
use crate::document::{DocumentLanguage, DocumentState};
use crate::utils::{get_project_files, is_relevant_file, mask_interpolations};
use apollo_compiler::executable;
use apollo_compiler::Schema;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub struct ScanResult {
    pub path: PathBuf,
    pub fragments: Vec<(String, String, Option<String>)>, // name, path, import_alias
    pub has_gql: bool,
    pub language: DocumentLanguage,
}

pub struct Engine;

impl Engine {
    /// Step 1: Discover all fragments across the entire workspace for cross-project imports
    pub fn scan_workspace(config: &Config) -> HashMap<String, (String, Option<String>)> {
        let mut fragment_map = HashMap::new();

        let all_scan_roots: Vec<_> = config
            .projects
            .iter()
            .map(|p| {
                (
                    config
                        .base_dir
                        .join(&p.include)
                        .to_string_lossy()
                        .to_string(),
                    p.import.clone(),
                )
            })
            .collect();

        let scan_results: Vec<Vec<_>> = all_scan_roots
            .par_iter()
            .map(|(abs_include, import_alias)| {
                let paths = get_project_files(abs_include);
                let mut results = Vec::new();
                for path in paths {
                    if is_relevant_file(&path) {
                        if let Some(doc) = Self::parse_doc(&path) {
                            for frag in doc.fragments() {
                                results.push((
                                    frag.name.clone(),
                                    path.to_string_lossy().to_string(),
                                    import_alias.clone(),
                                ));
                            }
                        }
                    }
                }
                results
            })
            .collect();

        for results in scan_results {
            for (name, path, alias) in results {
                fragment_map.insert(name, (path, alias));
            }
        }

        fragment_map
    }

    /// Step 1b: Discovery for simple mode (no config)
    pub fn scan_path(scan_path: &str) -> HashMap<String, String> {
        let mut fragment_map = HashMap::new();
        let paths = get_project_files(scan_path);

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
        let mut all_fragments = HashMap::new();

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

        // Check fast-path
        if language.is_host_language() {
            let upper = content.to_uppercase();
            if !upper.contains("GQL") && !upper.contains("GRAPHQL") {
                return None;
            }
        }

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language.get_parser_language()).ok()?;
        Some(DocumentState::new(uri, &content, parser))
    }

    pub fn load_schema(base_dir: &Path, source: &SchemaSource) -> Result<Schema, String> {
        let mut combined_text = String::new();
        for file in source.files() {
            let text = std::fs::read_to_string(base_dir.join(&file))
                .map_err(|e| format!("Failed to read schema file {}: {}", file, e))?;
            combined_text.push_str(&text);
            combined_text.push('\n');
        }
        Schema::parse(&combined_text, &source.as_key())
            .map_err(|e| format!("Failed to parse schema {}: {}", source.as_key(), e))
    }
}
