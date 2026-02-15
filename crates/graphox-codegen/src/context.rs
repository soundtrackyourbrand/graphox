use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::executable;
use apollo_compiler::schema::ExtendedType;
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
    pub type_cache: &'a SchemaAnalysisCaches,
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
        type_cache: &'a SchemaAnalysisCaches,
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
    /// Uses tuple-based key with default context for backward compatibility
    pub fn get_cached_type(&self, type_name: &str, compute: impl FnOnce() -> String) -> String {
        let key = TypeCacheKey {
            type_name: type_name.to_string(),
            use_names: false,
            schema_import_key: None,
            type_import_keys: Vec::new(),
        };
        self.type_cache.type_cache.get_or_insert_tuple(key, compute)
    }

    /// Get cached type with full context key (tuple-based)
    /// This ensures correctness when context settings vary between calls
    pub fn get_cached_type_with_context(
        &self,
        type_name: &str,
        use_names: bool,
        compute: impl FnOnce() -> String,
    ) -> String {
        let key =
            TypeCacheKey::from_context(type_name, use_names, self.schema_import, self.type_imports);
        self.type_cache.type_cache.get_or_insert_tuple(key, compute)
    }

    /// Get cached interface implementors
    pub fn get_interface_implementors(&self, interface_name: &str) -> Vec<String> {
        self.type_cache.interface_implementors.get_or_insert(
            self.schema_import,
            interface_name,
            || crate::helpers::compute_interface_implementors(interface_name, self.schema),
        )
    }

    /// Get cached abstract members (for unions/interfaces)
    pub fn get_abstract_members(&self, type_name: &str) -> Vec<String> {
        self.type_cache
            .abstract_members
            .get_or_insert(self.schema_import, type_name, || {
                crate::helpers::compute_abstract_members(type_name, self.schema)
            })
    }

    /// Get typename value for a type (uses cached interface implementors)
    /// Note: __typename is a GraphQL string field, so non-interface types need quotes
    /// Only Interface types use implementors; Union types fall through to default
    pub fn get_typename_value_for_type(&self, parent_type: &ExtendedType) -> String {
        match parent_type {
            ExtendedType::Interface(_) => {
                let implementors = self.get_interface_implementors(parent_type.name());
                if implementors.is_empty() {
                    "string".to_string()
                } else {
                    implementors.join(" | ")
                }
            }
            _ => format!("\"{}\"", parent_type.name()),
        }
    }
}

/// Cache key that includes context settings affecting type conversion
/// Using tuple-based struct for type safety and clarity
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeCacheKey {
    pub type_name: String,
    pub use_names: bool,
    pub schema_import_key: Option<String>,
    pub type_import_keys: Vec<String>,
}

impl TypeCacheKey {
    /// Create a fingerprint from context settings
    pub fn from_context(
        type_name: &str,
        use_names: bool,
        schema_import: &Option<String>,
        type_imports: &HashMap<String, String>,
    ) -> Self {
        let mut keys: Vec<_> = type_imports.keys().cloned().collect();
        keys.sort();
        Self {
            type_name: type_name.to_string(),
            use_names,
            schema_import_key: schema_import.clone(),
            type_import_keys: keys,
        }
    }
}

/// Thread-safe cache for GraphQL type to TypeScript type conversions
/// Shared across all files in a project since they use the same schema
#[derive(Debug)]
pub struct TypeCache {
    cache: DashMap<TypeCacheKey, String>,
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

    /// Get or insert using a tuple-based key for type-safe context-aware caching
    pub fn get_or_insert_tuple(
        &self,
        key: TypeCacheKey,
        compute: impl FnOnce() -> String,
    ) -> String {
        if let Some(cached) = self.cache.get(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return cached.clone();
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        let result = compute();
        self.cache.insert(key, result.clone());
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

/// Cache for interface → implementors mapping
/// Key: (schema_import, interface name), Value: list of implementor type names
#[derive(Debug, Default)]
pub struct InterfaceImplementorsCache {
    cache: DashMap<(Option<String>, String), Vec<String>>,
}

impl InterfaceImplementorsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert(
        &self,
        schema_import: &Option<String>,
        interface_name: &str,
        compute: impl FnOnce() -> Vec<String>,
    ) -> Vec<String> {
        let key = (schema_import.clone(), interface_name.to_string());
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }
        let result = compute();
        self.cache.insert(key, result.clone());
        result
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Cache for abstract type (union/interface) → members mapping
/// Key: (schema_import, type name), Value: list of member type names
#[derive(Debug, Default)]
pub struct AbstractMembersCache {
    cache: DashMap<(Option<String>, String), Vec<String>>,
}

impl AbstractMembersCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert(
        &self,
        schema_import: &Option<String>,
        type_name: &str,
        compute: impl FnOnce() -> Vec<String>,
    ) -> Vec<String> {
        let key = (schema_import.clone(), type_name.to_string());
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }
        let result = compute();
        self.cache.insert(key, result.clone());
        result
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Unified caches for schema analysis, shared at workspace level
#[derive(Debug, Default)]
pub struct SchemaAnalysisCaches {
    pub type_cache: TypeCache,
    pub interface_implementors: InterfaceImplementorsCache,
    pub abstract_members: AbstractMembersCache,
}

impl SchemaAnalysisCaches {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct OperationGenerated {
    pub name: String,
    pub operation_type_name: String,
    pub variables_type_name: String,
    pub document_name: String,
    pub source_text: String,
    pub codegen_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FragmentGenerated {
    pub name: String,
    pub fragment_type_name: String,
    pub source_text: String,
    pub document_name: String,
    pub codegen_path: PathBuf,
}

#[derive(Debug, Default, Clone)]
pub struct CodegenProfile {
    pub parse_time: std::time::Duration,
    pub selection_set_time: std::time::Duration,
    pub ast_serialization_time: std::time::Duration,
    pub import_generation_time: std::time::Duration,
}
