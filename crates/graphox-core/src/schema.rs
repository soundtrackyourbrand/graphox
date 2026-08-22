//! Schema loading and management utilities
//!
//! This module consolidates all schema-related operations that were previously
//! duplicated across backend.rs, engine.rs, and commands/codegen.rs

use crate::config::SchemaSource;
use ahash::AHashMap;
use apollo_compiler::Schema;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SloClass {
    Critical = 0,
    HighFast = 1,
    HighSlow = 2,
    Low = 3,
    NoSlo = 4,
}

impl std::str::FromStr for SloClass {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "CRITICAL" => Ok(SloClass::Critical),
            "HIGH_FAST" => Ok(SloClass::HighFast),
            "HIGH_SLOW" => Ok(SloClass::HighSlow),
            "LOW" => Ok(SloClass::Low),
            "NO_SLO" => Ok(SloClass::NoSlo),
            _ => Err(()),
        }
    }
}

impl SloClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            SloClass::Critical => "CRITICAL",
            SloClass::HighFast => "HIGH_FAST",
            SloClass::HighSlow => "HIGH_SLOW",
            SloClass::Low => "LOW",
            SloClass::NoSlo => "NO_SLO",
        }
    }

    pub fn worst(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

#[derive(Clone, Debug)]
pub struct SubgraphInfo {
    pub name: String,
    pub owner: Option<String>,
    pub schema: Arc<Schema>,
    pub uri: ls_types::Uri,
    /// Overall SLO class for the subgraph (from schema definition)
    pub schema_slo: Option<SloClass>,
    /// Maps TypeName -> FieldName -> SloClass
    pub field_slos: AHashMap<String, AHashMap<String, SloClass>>,
}

fn extract_slo_from_directives(
    directives: &[apollo_compiler::Node<apollo_compiler::ast::Directive>],
    schema: &Schema,
) -> Option<SloClass> {
    let slo_directive = directives.iter().find(|d| d.name == "slo")?;
    let class_arg = slo_directive.argument_by_name("class", schema).ok()?;

    // Handle both string and enum values
    let class_str = class_arg
        .as_str()
        .or_else(|| class_arg.as_enum().map(|e| e.as_str()))?;

    class_str.parse::<SloClass>().ok()
}

fn extract_slo_from_component_directives(
    directives: &apollo_compiler::schema::DirectiveList,
    schema: &Schema,
) -> Option<SloClass> {
    let slo_directive = directives.iter().find(|d| d.name == "slo")?;
    let class_arg = slo_directive.argument_by_name("class", schema).ok()?;

    // Handle both string and enum values
    let class_str = class_arg
        .as_str()
        .or_else(|| class_arg.as_enum().map(|e| e.as_str()))?;

    class_str.parse::<SloClass>().ok()
}

/// Load subgraphs from a directory
pub fn load_subgraphs(
    base_dir: &Path,
    subgraphs_dir: &str,
    owners: Option<&AHashMap<String, String>>,
) -> Vec<SubgraphInfo> {
    let mut subgraphs = Vec::new();
    let subgraphs_path = base_dir.join(subgraphs_dir);

    if let Ok(entries) = std::fs::read_dir(subgraphs_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "graphql")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                let schema = match Schema::parse(&content, &path) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("Failed to parse subgraph schema at {:?}: {}", path, e);
                        continue;
                    }
                };
                let mut field_slos = AHashMap::default();

                // Extract schema-level SLO
                let schema_slo = extract_slo_from_component_directives(
                    &schema.schema_definition.directives,
                    &schema,
                );

                // Extract SLO classes from the schema types
                for (type_name, type_def) in schema.types.iter() {
                    let mut type_fields_slo = AHashMap::default();

                    if let Some(obj) = type_def.as_object() {
                        for (field_name, field_def) in obj.fields.iter() {
                            if let Some(slo) =
                                extract_slo_from_directives(&field_def.directives, &schema)
                            {
                                type_fields_slo.insert(field_name.to_string(), slo);
                            }
                        }
                    } else if let Some(intf) = type_def.as_interface() {
                        for (field_name, field_def) in intf.fields.iter() {
                            if let Some(slo) =
                                extract_slo_from_directives(&field_def.directives, &schema)
                            {
                                type_fields_slo.insert(field_name.to_string(), slo);
                            }
                        }
                    }

                    if !type_fields_slo.is_empty() {
                        field_slos.insert(type_name.to_string(), type_fields_slo);
                    }
                }

                let owner = owners.and_then(|m| m.get(name).cloned());
                if let Some(uri) = crate::utils::path_to_uri(&path) {
                    subgraphs.push(SubgraphInfo {
                        name: name.to_string(),
                        owner,
                        schema: Arc::new(schema),
                        uri,
                        schema_slo,
                        field_slos,
                    });
                }
            }
        }
    }

    subgraphs
}

/// Load a schema from the given source files
///
/// This consolidates the schema loading logic that was previously duplicated in:
/// - Backend::load_schema_source (backend.rs:91-109)
/// - Engine::load_schema (engine.rs:370-390)
/// - Multiple locations in commands/codegen.rs
pub fn load_schema(base_dir: &Path, source: &SchemaSource) -> Result<Schema, String> {
    load_schema_with_cache(base_dir, source, true)
}

pub fn load_schema_no_cache(base_dir: &Path, source: &SchemaSource) -> Result<Schema, String> {
    load_schema_with_cache(base_dir, source, false)
}

/// Load a schema with optional caching
///
/// If `use_cache` is true, attempts to load from cache first and saves to cache after parsing.
/// This significantly speeds up repeated operations like benchmarks.
pub fn load_schema_with_cache(
    base_dir: &Path,
    source: &SchemaSource,
    use_cache: bool,
) -> Result<Schema, String> {
    // Try to load from cache if enabled
    let combined_text = if use_cache {
        if let Some(cached_text) = crate::schema_cache::try_load_from_cache(base_dir, source) {
            #[cfg(debug_assertions)]
            eprintln!("✓ Using cached schema for {}", source.as_key());
            cached_text
        } else {
            #[cfg(debug_assertions)]
            eprintln!("✗ Cache miss for {}, loading from disk", source.as_key());
            // Cache miss, load and merge files
            let merged = load_and_merge_schema_files(base_dir, source)?;
            // Save to cache for next time
            let _ = crate::schema_cache::save_to_cache(base_dir, source, &merged);
            merged
        }
    } else {
        // Cache disabled, just load and merge
        load_and_merge_schema_files(base_dir, source)?
    };

    Schema::parse(&combined_text, source.as_key())
        .map_err(|e| format!("Failed to parse schema {}: {}", source.as_key(), e))
}

/// Load and merge schema files without caching
fn load_and_merge_schema_files(base_dir: &Path, source: &SchemaSource) -> Result<String, String> {
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
    Ok(crate::utils::merge_schema_texts(&texts))
}

/// Load and validate a schema in one operation
///
/// This is a common pattern used throughout the codebase.
/// Uses a two-tier cache for maximum performance:
/// 1. Memory cache (L1): Fully parsed and validated schema
/// 2. Disk cache (L2): Merged schema text
///
/// Set `use_cache` to false to bypass all caching (useful for tests).
pub fn load_and_validate_schema(
    base_dir: &Path,
    source: &SchemaSource,
    use_cache: bool,
) -> Result<Arc<apollo_compiler::validation::Valid<Schema>>, String> {
    // Try memory cache first (L1) - fastest path, if caching enabled
    if use_cache
        && let Some(cached) = crate::schema_cache::try_load_parsed_from_memory(base_dir, source)
    {
        return Ok(cached);
    }

    #[cfg(debug_assertions)]
    if !use_cache {
        eprintln!(
            "✗ Cache disabled for {}, loading from disk",
            source.as_key()
        );
    } else {
        eprintln!("✗ Memory cache miss for {}", source.as_key());
    }

    // Memory cache miss, load and parse the schema
    let schema = load_schema_with_cache(base_dir, source, use_cache)?;

    // Validate the schema
    let validated = Arc::new(
        schema
            .validate()
            .map_err(|e| format!("Schema validation failed for {}: {}", source.as_key(), e))?,
    );

    // Save to memory cache for next time (if caching enabled)
    if use_cache {
        let _ = crate::schema_cache::save_parsed_to_memory(base_dir, source, validated.clone());
    }

    Ok(validated)
}

/// Load a schema and return it wrapped in Arc for shared ownership
///
/// Used in Backend::new and other initialization code
pub fn load_schema_arc(base_dir: &Path, source: &SchemaSource) -> Option<Arc<Schema>> {
    load_schema(base_dir, source).ok().map(Arc::new)
}
