pub mod benchmark;
pub mod check;
pub mod codegen;

pub use benchmark::run_benchmark;
pub use check::run_check;
pub use codegen::{CodegenParams, run_codegen};

use graphox_core::Config;
use graphox_core::config::SchemaSource;
use graphox_core::schema;
use rayon::prelude::*;
use std::sync::Arc;

pub(crate) type ValidSchema = Arc<apollo_compiler::validation::Valid<apollo_compiler::Schema>>;

/// Load + validate every distinct project schema once (in parallel), keyed by schema
/// key. Projects sharing a schema reuse the same validated schema instead of each
/// re-loading and re-validating the same (often large) schema. Errors are stored and
/// surfaced when the owning project is processed.
pub(crate) fn build_validated_schemas(
    config: &Config,
) -> ahash::AHashMap<String, Result<ValidSchema, String>> {
    let mut seen = ahash::AHashSet::default();
    let unique: Vec<(String, &SchemaSource)> = config
        .projects()
        .iter()
        .filter_map(|p| {
            let key = p.schema().as_key();
            seen.insert(key.clone()).then_some((key, p.schema()))
        })
        .collect();

    let pairs: Vec<(String, Result<ValidSchema, String>)> = unique
        .into_par_iter()
        .map(|(key, source)| {
            let result = schema::load_schema_with_cache(
                config.base_dir(),
                source,
                config.enable_schema_cache(),
            )
            .map_err(|e| format!("Failed to load schema {}: {}", source.as_key(), e))
            .and_then(|s| {
                s.validate()
                    .map(Arc::new)
                    .map_err(|e| format!("Invalid schema {}: {}", source.as_key(), e))
            });
            (key, result)
        })
        .collect();
    pairs.into_iter().collect()
}
