use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::executable;
use apollo_compiler::{Node, Schema};
use dashmap::DashMap;
use graphox_core::config::{CodegenConfig, EmitExtensions, NamingConvention};
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
    pub fn from_core_config(config: &graphox_core::config::FragmentMaskingConfig) -> Self {
        match config.mode() {
            graphox_core::config::FragmentMasking::Disabled => FragmentMasking::Disabled,
            graphox_core::config::FragmentMasking::Enabled {
                unmask_function_name,
            } => FragmentMasking::Enabled {
                unmask_function_name: unmask_function_name
                    .clone()
                    .unwrap_or_else(|| "getFragmentData".to_string()),
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
    pub all_fragments: &'a HashMap<Arc<str>, Node<executable::Fragment>>,
    pub current_file_path: &'a Path,
    pub scalars: &'a HashMap<String, String>,
    pub schema_import: &'a Option<String>,
    pub type_imports: &'a HashMap<String, String>,
    pub generate_ast_for_fragments: bool,
    pub fragment_dependencies: &'a HashMap<Arc<str>, Vec<Arc<str>>>,
    pub type_cache: &'a TypeCache,
    pub config: &'a CodegenConfig,
    pub masking_import_path: String,
    pub used_schema_types: RefCell<HashSet<String>>,
    pub codegen_path: PathBuf,
}

impl<'a> CodegenContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &'a apollo_compiler::validation::Valid<Schema>,
        fragment_to_path: &'a HashMap<Arc<str>, Arc<str>>,
        fragment_to_import: &'a HashMap<Arc<str>, Arc<str>>,
        fragment_to_type_only: &'a HashMap<Arc<str>, bool>,
        all_fragments: &'a HashMap<Arc<str>, Node<executable::Fragment>>,
        current_file_path: &'a Path,
        scalars: &'a HashMap<String, String>,
        schema_import: &'a Option<String>,
        type_imports: &'a HashMap<String, String>,
        generate_ast_for_fragments: bool,
        fragment_dependencies: &'a HashMap<Arc<str>, Vec<Arc<str>>>,
        type_cache: &'a TypeCache,
        config: &'a CodegenConfig,
        masking_import_path: String,
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
            config,
            masking_import_path,
            used_schema_types: RefCell::new(HashSet::new()),
            codegen_path,
        }
    }

    pub fn document_suffix(&self) -> &str {
        self.config.document_suffix()
    }

    pub fn variables_suffix(&self) -> &str {
        self.config.variables_suffix()
    }

    pub fn fragment_suffix(&self) -> &str {
        self.config.fragment_suffix()
    }

    pub fn fragment_document_suffix(&self) -> &str {
        self.config.fragment_document_suffix()
    }

    pub fn query_suffix(&self) -> &str {
        self.config.query_suffix()
    }

    pub fn mutation_suffix(&self) -> &str {
        self.config.mutation_suffix()
    }

    pub fn subscription_suffix(&self) -> &str {
        self.config.subscription_suffix()
    }

    pub fn naming_convention(&self) -> NamingConvention {
        self.config.naming_convention()
    }

    pub fn fragment_masking(&self) -> FragmentMasking {
        FragmentMasking::from_core_config(&self.config.fragment_masking())
    }

    pub fn emit_extensions(&self) -> EmitExtensions {
        self.config.emit_extensions()
    }

    pub fn re_exports(&self) -> bool {
        self.config.re_exports()
    }

    /// Get cached type conversion or compute and cache it
    pub fn get_cached_type(&self, type_name: &str, compute: impl FnOnce() -> String) -> String {
        self.type_cache.get_or_insert(type_name, compute)
    }
}

/// Thread-safe cache for GraphQL type to TypeScript type conversions
/// Shared across all files in a project since they use the same schema
pub struct TypeCache {
    cache: DashMap<String, String>,
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
