pub mod analyze;
pub mod benchmark;
pub mod check;
pub mod codegen;

pub use analyze::{AnalyzeParams, run_analyze};
pub use benchmark::run_benchmark;
pub use check::run_check;
pub use codegen::{CodegenParams, run_codegen};

use ahash::AHashSet;
use graphox_core::Config;
use graphox_core::config::SchemaSource;
use graphox_core::engine::WorkspaceMetadata;
use graphox_core::schema;
use rayon::prelude::*;
use std::path::PathBuf;
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

/// Group every scanned file by the schema its project uses.
///
/// Cross-document analysis is only meaningful within one schema — two projects
/// on different schemas could never share a fragment — and several projects
/// usually share one, so this is the unit such an analysis runs over.
pub(crate) fn documents_by_schema(
    config: &Config,
    workspace: &WorkspaceMetadata,
) -> Vec<(String, Vec<(usize, PathBuf)>)> {
    let mut by_schema: Vec<(String, Vec<(usize, PathBuf)>)> = Vec::new();
    for (idx, (project, meta)) in config
        .projects()
        .iter()
        .zip(&workspace.projects)
        .enumerate()
    {
        let key = project.schema().as_key();
        let entry = match by_schema.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry,
            None => {
                by_schema.push((key, Vec::new()));
                by_schema.last_mut().expect("just pushed")
            }
        };
        for file in &meta.files {
            entry.1.push((idx, file.clone()));
        }
    }
    by_schema
}

/// Fields the configuration mandates. They are present because graphox put them
/// there, so they say nothing about how a selection was written.
pub(crate) fn mandated_fields(config: &Config) -> AHashSet<String> {
    let mut fields: AHashSet<String> = config.rules().required_fields().keys().cloned().collect();
    fields.insert("__typename".to_string());
    fields
}
