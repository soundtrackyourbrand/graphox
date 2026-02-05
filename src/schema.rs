//! Schema loading and management utilities
//!
//! This module consolidates all schema-related operations that were previously
//! duplicated across backend.rs, engine.rs, and commands/codegen.rs

use crate::config::SchemaSource;
use apollo_compiler::Schema;
use std::path::Path;
use std::sync::Arc;

/// Load a schema from the given source files
///
/// This consolidates the schema loading logic that was previously duplicated in:
/// - Backend::load_schema_source (backend.rs:91-109)
/// - Engine::load_schema (engine.rs:370-390)
/// - Multiple locations in commands/codegen.rs
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

/// Load and validate a schema in one operation
///
/// This is a common pattern used throughout the codebase
pub fn load_and_validate_schema(
    base_dir: &Path,
    source: &SchemaSource,
) -> Result<apollo_compiler::validation::Valid<Schema>, String> {
    let schema = load_schema(base_dir, source)?;
    schema
        .validate()
        .map_err(|e| format!("Schema validation failed for {}: {}", source.as_key(), e))
}

/// Load a schema and return it wrapped in Arc for shared ownership
///
/// Used in Backend::new and other initialization code
pub fn load_schema_arc(base_dir: &Path, source: &SchemaSource) -> Option<Arc<Schema>> {
    load_schema(base_dir, source).ok().map(Arc::new)
}
