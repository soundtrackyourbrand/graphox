use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::executable;
use apollo_compiler::{Node, Schema};
use dashmap::DashMap;
use graphox_core::config::{EmitExtensions, FragmentMaskingConfig};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone)]
pub enum FragmentMasking {
    Disabled,
    Enabled { unmask_function_name: String },
}

impl FragmentMasking {
    pub fn from_config(config: &Option<FragmentMaskingConfig>) -> Self {
        match config {
            None => FragmentMasking::Disabled,
            Some(c) => match &c.mode {
                graphox_core::config::FragmentMasking::Disabled => FragmentMasking::Disabled,
                graphox_core::config::FragmentMasking::Enabled {
                    unmask_function_name,
                } => FragmentMasking::Enabled {
                    unmask_function_name: unmask_function_name
                        .clone()
                        .unwrap_or_else(|| "getFragmentData".to_string()),
                },
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self, FragmentMasking::Enabled { .. })
    }

    pub fn unmask_function_name(&self) -> &str {
        match self {
            FragmentMasking::Disabled => "getFragmentData",
            FragmentMasking::Enabled {
                unmask_function_name,
            } => unmask_function_name.as_str(),
        }
    }
}

pub struct CodegenContext<'a> {
    pub schema: &'a apollo_compiler::validation::Valid<Schema>,
    pub fragment_to_path: &'a HashMap<Arc<str>, Arc<str>>,
    pub fragment_to_import: &'a HashMap<Arc<str>, Arc<str>>,
    pub fragment_to_type_only: &'a HashMap<Arc<str>, bool>,
    pub all_fragments: &'a HashMap<String, Node<executable::Fragment>>,
    pub current_file_path: &'a Path,
    pub scalars: &'a Option<HashMap<String, String>>,
    pub schema_import: &'a Option<String>,
    pub type_imports: &'a HashMap<String, String>,
    pub generate_ast_for_fragments: bool,
    pub fragment_dependencies: &'a HashMap<Arc<str>, Vec<Arc<str>>>,
    pub type_cache: &'a TypeCache,
    pub document_suffix: &'a str,
    pub variables_suffix: &'a str,
    pub fragment_suffix: &'a str,
    pub fragment_document_suffix: &'a str,
    pub query_suffix: &'a str,
    pub mutation_suffix: &'a str,
    pub subscription_suffix: &'a str,
    pub fragment_masking: FragmentMasking,
    pub masking_import_path: String,
    pub used_schema_types: RefCell<HashSet<String>>,
    pub emit_extensions: EmitExtensions,
    pub codegen_path: PathBuf,
}

/// Thread-safe cache for GraphQL type to TypeScript type conversions
/// Shared across all files in a project since they use the same schema
pub struct TypeCache {
    cache: DashMap<String, String>,
    // Optional metrics for benchmarking
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl Default for TypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeCache {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    pub fn get_or_insert(&self, key: &str, compute: impl FnOnce() -> String) -> String {
        if let Some(cached) = self.cache.get(key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return cached.clone();
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        let result = compute();
        self.cache.insert(key.to_string(), result.clone());
        result
    }

    pub fn stats(&self) -> (usize, usize) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl<'a> CodegenContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &'a apollo_compiler::validation::Valid<Schema>,
        fragment_to_path: &'a HashMap<Arc<str>, Arc<str>>,
        fragment_to_import: &'a HashMap<Arc<str>, Arc<str>>,
        fragment_to_type_only: &'a HashMap<Arc<str>, bool>,
        all_fragments: &'a HashMap<String, Node<executable::Fragment>>,
        current_file_path: &'a Path,
        scalars: &'a Option<HashMap<String, String>>,
        schema_import: &'a Option<String>,
        type_imports: &'a HashMap<String, String>,
        generate_ast_for_fragments: bool,
        fragment_dependencies: &'a HashMap<Arc<str>, Vec<Arc<str>>>,
        type_cache: &'a TypeCache,
        document_suffix: &'a str,
        variables_suffix: &'a str,
        fragment_suffix: &'a str,
        fragment_document_suffix: &'a str,
        query_suffix: &'a str,
        mutation_suffix: &'a str,
        subscription_suffix: &'a str,
        fragment_masking: FragmentMasking,
        masking_import_path: String,
        emit_extensions: EmitExtensions,
        codegen_path: PathBuf,
    ) -> Self {
        Self {
            schema,
            fragment_to_path,
            fragment_to_import,
            fragment_to_type_only,
            all_fragments,
            current_file_path,
            scalars,
            schema_import,
            type_imports,
            generate_ast_for_fragments,
            fragment_dependencies,
            type_cache,
            document_suffix,
            variables_suffix,
            fragment_suffix,
            fragment_document_suffix,
            query_suffix,
            mutation_suffix,
            subscription_suffix,
            fragment_masking,
            masking_import_path,
            emit_extensions,
            used_schema_types: RefCell::new(HashSet::new()),
            codegen_path,
        }
    }

    /// Get cached type conversion or compute and cache it
    pub fn get_cached_type(&self, type_name: &str, compute: impl FnOnce() -> String) -> String {
        self.type_cache.get_or_insert(type_name, compute)
    }
}

#[derive(Debug, Clone)]
pub struct OperationGenerated {
    pub name: String,
    pub operation_type_name: String,
    pub variables_type_name: String,
    pub source_text: String,
    pub codegen_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FragmentGenerated {
    pub name: String,
    pub source_text: String,
    pub document_name: String,
    pub codegen_path: PathBuf,
}

#[derive(Debug, Default)]
pub struct CodegenProfile {
    pub parse_time: std::time::Duration,
    pub selection_set_time: std::time::Duration,
    pub ast_serialization_time: std::time::Duration,
    pub import_generation_time: std::time::Duration,
}
