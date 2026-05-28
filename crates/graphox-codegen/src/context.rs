use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::ast::OperationType;
use apollo_compiler::executable;
use apollo_compiler::schema::ExtendedType;
use apollo_compiler::{Node, Schema};
use dashmap::DashMap;
use graphox_core::config::{CodegenConfig, EmitExtensions, NamingConvention};
use graphox_core::document::{FragmentId, TransitiveDeps};
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
    pub fragment_to_path: &'a HashMap<FragmentId, Arc<str>>,
    pub fragment_to_import: &'a HashMap<FragmentId, Arc<str>>,
    pub fragment_to_type_only: &'a HashMap<FragmentId, bool>,
    pub all_fragments: &'a HashMap<Arc<str>, Node<executable::Fragment>>,
    pub name_to_id: &'a HashMap<Arc<str>, FragmentId>,
    pub current_file_path: &'a Path,
    pub scalars: &'a HashMap<String, String>,
    pub schema_import: &'a Option<String>,
    pub type_imports: &'a HashMap<String, String>,
    pub generate_ast_for_fragments: bool,
    pub fragment_dependencies: &'a HashMap<FragmentId, TransitiveDeps>,
    pub type_cache: &'a SchemaAnalysisCaches,
    pub config: &'a CodegenConfig,
    pub masking_import_path: String,
    pub used_schema_types: RefCell<HashSet<String>>,
    pub codegen_path: PathBuf,
    pub context_fingerprint: u64,
}

impl<'a> CodegenContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &'a apollo_compiler::validation::Valid<Schema>,
        fragment_to_path: &'a HashMap<FragmentId, Arc<str>>,
        fragment_to_import: &'a HashMap<FragmentId, Arc<str>>,
        fragment_to_type_only: &'a HashMap<FragmentId, bool>,
        all_fragments: &'a HashMap<Arc<str>, Node<executable::Fragment>>,
        name_to_id: &'a HashMap<Arc<str>, FragmentId>,
        current_file_path: &'a Path,
        scalars: &'a HashMap<String, String>,
        schema_import: &'a Option<String>,
        type_imports: &'a HashMap<String, String>,
        generate_ast_for_fragments: bool,
        fragment_dependencies: &'a HashMap<FragmentId, TransitiveDeps>,
        type_cache: &'a SchemaAnalysisCaches,
        config: &'a CodegenConfig,
        masking_import_path: String,
        codegen_path: PathBuf,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        schema_import.hash(&mut hasher);
        config.fingerprint().hash(&mut hasher);
        let mut sorted_keys: Vec<_> = type_imports.keys().collect();
        sorted_keys.sort_unstable();
        for k in sorted_keys {
            k.hash(&mut hasher);
            type_imports.get(k).unwrap().hash(&mut hasher);
        }
        let context_fingerprint = hasher.finish();

        Self {
            schema,
            fragment_to_path,
            fragment_to_import,
            fragment_to_type_only,
            all_fragments,
            name_to_id,
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
            context_fingerprint,
        }
    }

    pub fn document_suffix(&self) -> &str {
        self.config.document_suffix()
    }

    pub fn omit_operation_suffix_in_document_name(&self) -> bool {
        self.config.omit_operation_suffix_in_document_name()
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

    pub fn react_apollo_hooks(&self) -> bool {
        self.config.react_apollo_hooks()
    }

    pub fn apollo_react_common_import_from(&self) -> &str {
        self.config.apollo_react_common_import_from()
    }

    pub fn apollo_react_hooks_import_from(&self) -> &str {
        self.config.apollo_react_hooks_import_from()
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

    pub fn nullable_fields_as_optional(&self) -> bool {
        self.config.nullable_fields_as_optional()
    }

    pub fn merge_union_types(&self) -> bool {
        self.config.merge_union_types()
    }

    /// Get relative path from cache or compute it
    pub fn diff_paths(&self, from: &Path, to: &Path) -> Option<PathBuf> {
        let key = (from.to_path_buf(), to.to_path_buf());
        if let Some(cached) = self.type_cache.diff_path_cache.get(&key) {
            return cached.clone();
        }

        let res = pathdiff::diff_paths(from, to);
        self.type_cache.diff_path_cache.insert(key, res.clone());
        res
    }

    /// Get canonical path from cache or compute it
    pub fn canonicalize_path(&self, path: &Path) -> PathBuf {
        if let Some(cached) = self.type_cache.canonical_path_cache.get(path) {
            return cached.clone();
        }

        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.type_cache
            .canonical_path_cache
            .insert(path.to_path_buf(), canonical.clone());
        canonical
    }

    /// Get final import path from cache or compute it
    pub fn get_final_import_path(&self, fragment_path: &Arc<str>, parent_dir: &Path) -> String {
        let key = (fragment_path.clone(), parent_dir.to_path_buf());
        if let Some(cached) = self.type_cache.final_import_path_cache.get(&key) {
            return cached.clone();
        }

        let abs_fragment_path = self.canonicalize_path(Path::new(fragment_path.as_ref()));
        let abs_parent_dir = self.canonicalize_path(parent_dir);
        let rel_path = self
            .diff_paths(&abs_fragment_path, &abs_parent_dir)
            .unwrap_or_else(|| abs_fragment_path.clone());

        let mut path_str = graphox_core::utils::to_posix_path(&rel_path);
        if !path_str.starts_with('.') && !rel_path.is_absolute() {
            path_str.insert_str(0, "./");
        }
        let p = Path::new(&path_str);
        let stem = p.file_stem().unwrap().to_str().unwrap();
        let parent = p.parent().unwrap();
        let final_p = parent.join(stem);
        let mut final_path_str = graphox_core::utils::to_posix_path(&final_p);
        if !final_path_str.starts_with('.')
            && !final_path_str.starts_with('/')
            && !final_p.is_absolute()
        {
            final_path_str.insert_str(0, "./");
        }
        final_path_str.push_str(".codegen");
        final_path_str.push_str(self.emit_extensions().as_str());

        self.type_cache
            .final_import_path_cache
            .insert(key, final_path_str.clone());
        final_path_str
    }

    /// Get cached fragment AST JSON string
    pub fn get_fragment_ast(&self, fragment_name: &Arc<str>) -> Option<Arc<str>> {
        let key = (fragment_name.clone(), self.context_fingerprint);
        self.type_cache
            .fragment_ast_cache
            .get(&key)
            .map(|r| r.value().clone())
    }

    /// Insert fragment AST JSON string into cache
    pub fn insert_fragment_ast(&self, fragment_name: Arc<str>, ast_json: Arc<str>) {
        let key = (fragment_name, self.context_fingerprint);
        self.type_cache.fragment_ast_cache.insert(key, ast_json);
    }

    /// Get cached type conversion or compute and cache it
    /// Uses tuple-based key with default context for backward compatibility
    pub fn get_cached_type(&self, type_name: &str, compute: impl FnOnce() -> String) -> String {
        let key = TypeCacheKey {
            type_name: Arc::from(type_name),
            use_names: false,
            context_fingerprint: self.context_fingerprint,
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
        let key = TypeCacheKey {
            type_name: Arc::from(type_name),
            use_names,
            context_fingerprint: self.context_fingerprint,
        };
        self.type_cache.type_cache.get_or_insert_tuple(key, compute)
    }

    /// Get cached interface implementors
    pub fn get_interface_implementors(&self, interface_name: &str) -> Vec<Arc<str>> {
        self.type_cache.interface_implementors.get_or_insert(
            self.schema_import,
            interface_name,
            || crate::helpers::compute_interface_implementors(interface_name, self.schema),
        )
    }

    /// Get cached abstract members (for unions/interfaces)
    pub fn get_abstract_members(&self, type_name: &str) -> Vec<Arc<str>> {
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
                    implementors
                        .iter()
                        .map(|m| format!("\"{}\"", m))
                        .collect::<Vec<_>>()
                        .join(" | ")
                }
            }
            _ => format!("\"{}\"", parent_type.name()),
        }
    }

    pub fn get_abstract_members_intersection(&self, type_a: &str, type_b: &str) -> Vec<Arc<str>> {
        let members_a = self.get_abstract_members(type_a);
        let members_b = self.get_abstract_members(type_b);

        let set_b: HashSet<Arc<str>> = members_b.into_iter().collect();
        members_a
            .into_iter()
            .filter(|m| set_b.contains(m))
            .collect()
    }

    pub fn get_typename_value_for_type_with_context(
        &self,
        current_type: &ExtendedType,
        expected_type: &ExtendedType,
    ) -> String {
        let intersection =
            self.get_abstract_members_intersection(current_type.name(), expected_type.name());
        if intersection.is_empty() {
            // This shouldn't happen if we filter branches correctly, but as a fallback:
            self.get_typename_value_for_type(current_type)
        } else {
            intersection
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(" | ")
        }
    }

    /// Check if a concrete type matches or implements a target type condition
    pub fn is_type_applicable(&self, member_type_name: &str, target_type_name: &str) -> bool {
        if member_type_name == target_type_name {
            return true;
        }
        let members = self.get_abstract_members(target_type_name);
        members.iter().any(|m| m.as_ref() == member_type_name)
    }
}

/// Cache key that includes context settings affecting type conversion
/// Using tuple-based struct for type safety and clarity
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeCacheKey {
    pub type_name: Arc<str>,
    pub use_names: bool,
    pub context_fingerprint: u64,
}

impl TypeCacheKey {
    /// Create a fingerprint from context settings
    pub fn from_context(
        type_name: &str,
        use_names: bool,
        schema_import: &Option<String>,
        type_imports: &HashMap<String, String>,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        schema_import.hash(&mut hasher);
        let mut sorted_keys: Vec<_> = type_imports.keys().collect();
        sorted_keys.sort_unstable();
        for k in sorted_keys {
            k.hash(&mut hasher);
            type_imports.get(k).unwrap().hash(&mut hasher);
        }
        let fingerprint = hasher.finish();

        Self {
            type_name: Arc::from(type_name),
            use_names,
            context_fingerprint: fingerprint,
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
    cache: DashMap<(Option<String>, String), Vec<Arc<str>>>,
}

impl InterfaceImplementorsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert(
        &self,
        schema_import: &Option<String>,
        interface_name: &str,
        compute: impl FnOnce() -> Vec<Arc<str>>,
    ) -> Vec<Arc<str>> {
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
    cache: DashMap<(Option<String>, String), Vec<Arc<str>>>,
}

impl AbstractMembersCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert(
        &self,
        schema_import: &Option<String>,
        type_name: &str,
        compute: impl FnOnce() -> Vec<Arc<str>>,
    ) -> Vec<Arc<str>> {
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
pub struct SchemaAnalysisCaches {
    pub type_cache: TypeCache,
    pub interface_implementors: InterfaceImplementorsCache,
    pub abstract_members: AbstractMembersCache,
    pub canonical_path_cache: DashMap<PathBuf, PathBuf>,
    pub diff_path_cache: DashMap<(PathBuf, PathBuf), Option<PathBuf>>,
    pub final_import_path_cache: DashMap<(Arc<str>, PathBuf), String>,
    pub fragment_ast_cache: DashMap<(Arc<str>, u64), Arc<str>>,
}

impl SchemaAnalysisCaches {
    pub fn new() -> Self {
        Self {
            type_cache: TypeCache::new(),
            interface_implementors: InterfaceImplementorsCache::new(),
            abstract_members: AbstractMembersCache::new(),
            canonical_path_cache: DashMap::new(),
            diff_path_cache: DashMap::new(),
            final_import_path_cache: DashMap::new(),
            fragment_ast_cache: DashMap::new(),
        }
    }
}

impl Default for SchemaAnalysisCaches {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct OperationGenerated {
    pub name: String,
    pub operation_type: OperationType,
    pub operation_type_name: String,
    pub variables_type_name: String,
    pub document_name: String,
    pub hook_names: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use apollo_compiler::Schema;
    use graphox_core::config::CodegenConfig;
    use std::cell::RefCell;

    #[test]
    fn test_diff_paths_caching() {
        let caches = SchemaAnalysisCaches::new();
        let schema_src = r#"
        type Query {
          foo: String
        }
        "#;
        let schema = Schema::parse(schema_src, "schema.graphql").unwrap();
        let valid_schema = schema.validate().expect("Schema should be valid");

        // Mock dependencies
        let fragment_to_path = HashMap::new();
        let fragment_to_import = HashMap::new();
        let fragment_to_type_only = HashMap::new();
        let all_fragments = HashMap::new();
        let current_file_path = Path::new("/a/b/c.ts");
        let scalars = HashMap::new();
        let schema_import = None;
        let type_imports = HashMap::new();
        let fragment_dependencies = HashMap::new();
        let config = CodegenConfig::default();
        let used_schema_types = RefCell::new(HashSet::new());
        let codegen_path = PathBuf::from("/a/b/c.ts");
        let masking_import_path = "".to_string();

        let name_to_id = HashMap::default();

        let ctx = CodegenContext {
            schema: &valid_schema,
            fragment_to_path: &fragment_to_path,
            fragment_to_import: &fragment_to_import,
            fragment_to_type_only: &fragment_to_type_only,
            all_fragments: &all_fragments,
            name_to_id: &name_to_id,
            current_file_path,
            scalars: &scalars,
            schema_import: &schema_import,
            type_imports: &type_imports,
            generate_ast_for_fragments: false,
            fragment_dependencies: &fragment_dependencies,
            type_cache: &caches,
            config: &config,
            masking_import_path,
            used_schema_types,
            codegen_path,
            context_fingerprint: 0,
        };

        let from = Path::new("/a/b/x/file.ts");
        let to = Path::new("/a/b");

        // First call - should calculate and cache
        let res1 = ctx.diff_paths(from, to);
        assert!(res1.is_some());

        // Verify cache is populated
        assert_eq!(caches.diff_path_cache.len(), 1);

        // Second call - should use cache
        let res2 = ctx.diff_paths(from, to);
        assert_eq!(res1, res2);

        // Verify cache size unchanged
        assert_eq!(caches.diff_path_cache.len(), 1);

        // Third call - different path
        let from2 = Path::new("/a/c/y/file.ts");
        let res3 = ctx.diff_paths(from2, to);
        assert!(res3.is_some());
        assert_ne!(res1, res3);

        // Verify cache grew
        assert_eq!(caches.diff_path_cache.len(), 2);
    }
}
