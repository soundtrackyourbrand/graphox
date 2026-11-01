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
            let segments = load_and_merge_schema_segments(base_dir, source)?;
            // Save to cache for next time
            let _ = crate::schema_cache::save_segments_to_cache(base_dir, source, &segments);
            join_segments(&segments)
        }
    } else {
        // Cache disabled, just load and merge
        join_segments(&load_and_merge_schema_segments(base_dir, source)?)
    };

    Schema::parse(&combined_text, source.as_key())
        .map_err(|e| format!("Failed to parse schema {}: {}", source.as_key(), e))
}

/// Loads several schemas, parsing what they share only once.
///
/// Projects often combine one large schema with a small one of their own, so
/// the large one opens many sources. Each source is merged as
/// [`load_schema_with_cache`] merges it, but parsed per file
/// ([`crate::utils::merge_schema_segments`]): a file that merges into the same
/// text in several sources is parsed once and its AST shared. The results are
/// in the order of `sources`.
///
/// With `use_cache`, a source merged on an earlier run comes from the schema
/// cache, as in [`load_schema_with_cache`], and one merged now is saved there.
/// Each other file is read and outlined for merging once, and every source merged
/// before anything is parsed, so each step runs in parallel without one task
/// waiting on another.
pub fn load_schemas(
    base_dir: &Path,
    sources: &[&SchemaSource],
    use_cache: bool,
) -> Vec<Result<Schema, String>> {
    use crate::utils::SchemaFileOutline;
    use rayon::prelude::*;

    let cached: Vec<Option<Vec<String>>> = sources
        .par_iter()
        .map(|source| {
            use_cache
                .then(|| crate::schema_cache::try_load_segments_from_cache(base_dir, source))
                .flatten()
        })
        .collect();

    // Each distinct file the cache did not cover, read and outlined once.
    let mut seen_files = ahash::AHashSet::default();
    let distinct_files: Vec<String> = sources
        .iter()
        .zip(&cached)
        .filter(|(_, cached)| cached.is_none())
        .flat_map(|(source, _)| source.files())
        .filter(|file| seen_files.insert(file.clone()))
        .collect();
    let files: std::collections::HashMap<String, Result<(String, SchemaFileOutline), String>> =
        distinct_files
            .into_par_iter()
            .map(|file| {
                let path = base_dir.join(&file);
                let read = std::fs::read_to_string(&path)
                    .map(|text| {
                        let outline = SchemaFileOutline::new(&text);
                        (text, outline)
                    })
                    .map_err(|e| format!("Failed to read schema file {}: {}", path.display(), e));
                (file, read)
            })
            .collect();

    /// Each file of a source, with the segment it merged into.
    type Merged = Result<Vec<(String, Arc<str>)>, String>;
    let merged: Vec<Merged> = sources
        .par_iter()
        .zip(cached)
        .map(|(source, cached)| {
            let names = source.files();
            let segments = match cached {
                Some(segments) => segments,
                None => {
                    let outlined = names
                        .iter()
                        .map(|file| {
                            let (text, outline) = files[file].as_ref().map_err(Clone::clone)?;
                            Ok((text.as_str(), outline))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    let segments = crate::utils::merge_outlined_schema_segments(&outlined);
                    if use_cache {
                        let _ = crate::schema_cache::save_segments_to_cache(
                            base_dir, source, &segments,
                        );
                    }
                    segments
                }
            };
            Ok(names
                .into_iter()
                .zip(segments.into_iter().map(Arc::from))
                .collect())
        })
        .collect();

    // Each distinct segment, with the first file it came from to name it by.
    let mut seen = ahash::AHashSet::default();
    let distinct: Vec<(Arc<str>, &str)> = merged
        .iter()
        .flatten()
        .flatten()
        .filter(|(_, segment)| seen.insert(segment.clone()))
        .map(|(file, segment)| (segment.clone(), file.as_str()))
        .collect();
    // A std map because that is what rayon can collect a parallel iterator into.
    let parsed: std::collections::HashMap<_, _> = distinct
        .into_par_iter()
        .map(|(segment, file)| {
            let document = apollo_compiler::ast::Document::parse(&*segment, file)
                .map_err(|e| e.errors.to_string());
            (segment, document)
        })
        .collect();

    sources
        .par_iter()
        .zip(merged)
        .map(|(source, segments)| {
            let parse_error = |e: &dyn std::fmt::Display| {
                format!("Failed to parse schema {}: {e}", source.as_key())
            };
            let mut builder = Schema::builder();
            for (_, segment) in segments? {
                let document: &Result<apollo_compiler::ast::Document, String> = &parsed[&segment];
                builder = builder.add_ast(document.as_ref().map_err(|e| parse_error(e))?);
            }
            builder.build().map_err(|e| parse_error(&e))
        })
        .collect()
}

/// The merged text [`crate::utils::merge_schema_texts`] makes of `segments`.
fn join_segments(segments: &[String]) -> String {
    let mut merged = String::with_capacity(segments.iter().map(|s| s.len() + 1).sum());
    for segment in segments {
        merged.push_str(segment);
        merged.push('\n');
    }
    merged
}

/// Load schema files and merge them into segments, without caching
fn load_and_merge_schema_segments(
    base_dir: &Path,
    source: &SchemaSource,
) -> Result<Vec<String>, String> {
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
    Ok(crate::utils::merge_schema_segments(&texts))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ntest::timeout(10000)]
    fn test_load_schemas_builds_what_the_merged_text_parses_to() {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            (
                "base.graphqls",
                "schema { query: Query } type Query { foo: String } scalar Date",
            ),
            (
                "again.graphqls",
                "schema { query: Query } type Query { bar: String } scalar Date",
            ),
            (
                "own.graphqls",
                "schema { query: Q2 } type Q2 { f2: String } type Query { baz: Int }",
            ),
        ];
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let source =
            |names: &[&str]| SchemaSource::Multiple(names.iter().map(|n| n.to_string()).collect());

        // The base file opens most sources, so they share its segment.
        let sources: Vec<_> = [
            &["base.graphqls"][..],
            &["base.graphqls", "again.graphqls"],
            &["base.graphqls", "own.graphqls"],
            &["base.graphqls", "again.graphqls", "own.graphqls"],
            &["own.graphqls"],
        ]
        .into_iter()
        .map(source)
        .collect();
        let loaded = load_schemas(dir.path(), &sources.iter().collect::<Vec<_>>(), false);
        for (source, loaded) in sources.iter().zip(loaded) {
            let merged = load_schema_no_cache(dir.path(), source).unwrap();
            let loaded = loaded.unwrap();
            assert_eq!(
                merged.to_string(),
                loaded.to_string(),
                "{}",
                source.as_key()
            );
            assert!(loaded.validate().is_ok(), "{}", source.as_key());
        }
    }

    #[test]
    #[ntest::timeout(10000)]
    fn test_load_schemas_reuses_what_the_cache_holds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("base.graphqls"), "type Query { a: Int }").unwrap();
        std::fs::write(dir.path().join("own.graphqls"), "type Query { b: Int }").unwrap();
        let source = SchemaSource::Multiple(vec!["base.graphqls".into(), "own.graphqls".into()]);
        let expected = load_schema_no_cache(dir.path(), &source)
            .unwrap()
            .to_string();

        let first = load_schemas(dir.path(), &[&source], true)
            .remove(0)
            .unwrap();
        assert_eq!(first.to_string(), expected);
        assert!(crate::schema_cache::try_load_segments_from_cache(dir.path(), &source).is_some());

        // Change a file but keep its time, which the cache checks entries by:
        // only a load served from the cache still sees the old field.
        let own = dir.path().join("own.graphqls");
        let modified = std::fs::metadata(&own).unwrap().modified().unwrap();
        std::fs::write(&own, "type Query { c: Int }").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&own)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let second = load_schemas(dir.path(), &[&source], true)
            .remove(0)
            .unwrap();
        assert_eq!(second.to_string(), expected);
        let uncached = load_schemas(dir.path(), &[&source], false)
            .remove(0)
            .unwrap();
        assert_ne!(uncached.to_string(), expected);
    }

    #[test]
    #[ntest::timeout(10000)]
    fn test_load_schemas_reports_what_does_not_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.graphqls"), "type Query { foo: }").unwrap();
        let source = SchemaSource::Single("broken.graphqls".to_string());

        let missing = SchemaSource::Single("missing.graphqls".to_string());

        let mut results = load_schemas(dir.path(), &[&source, &missing], false).into_iter();
        let err = results.next().unwrap().unwrap_err();
        assert!(
            err.contains("Failed to parse schema broken.graphqls"),
            "{err}"
        );
        let err = results.next().unwrap().unwrap_err();
        assert!(err.contains("Failed to read schema file"), "{err}");
    }
}
