pub mod analyze;
pub mod benchmark;
pub mod check;
pub mod codegen;
pub mod codegen_weight;
pub mod operations;
pub(crate) mod setup;
pub mod usage;

pub use analyze::{AnalyzeParams, run_analyze};
pub use benchmark::run_benchmark;
pub use check::run_check;
pub use codegen::{CodegenParams, run_codegen};
pub use codegen_weight::{CodegenWeightParams, run_codegen_weight};
pub use operations::{OperationsParams, run_operations};
pub use usage::{UsageParams, run_usage};

use ahash::{AHashMap, AHashSet};
use colored::*;
use graphox_core::Config;
use graphox_core::config::SchemaSource;
use graphox_core::engine::{Engine, ProjectContext, WorkspaceMetadata};
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

/// Fields the configuration mandates, per project index.
///
/// `required_fields` can be overridden per project, and four projects turning
/// it off is not unusual. A field a project does not mandate was chosen by
/// whoever wrote the selection, so it has to be counted there even when another
/// project has it imposed — which means this cannot be read from the global
/// rules alone.
pub(crate) fn mandated_fields_by_project(config: &Config) -> AHashMap<usize, AHashSet<String>> {
    config
        .projects()
        .iter()
        .enumerate()
        .map(|(idx, project)| {
            let rules = match project.rules() {
                Some(project_rules) => config.rules().merge(project_rules),
                None => config.rules().clone(),
            };
            let mut fields: AHashSet<String> = rules.required_fields().keys().cloned().collect();
            // Never written by hand, so it never describes a selection.
            fields.insert("__typename".to_string());
            (idx, fields)
        })
        .collect()
}

/// `count noun`, pluralised.
pub(crate) fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// The first `limit` items, or all of them when it is zero.
pub(crate) fn take<T>(items: Vec<T>, limit: usize) -> Vec<T> {
    if limit == 0 {
        items
    } else {
        items.into_iter().take(limit).collect()
    }
}

pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn json_strings(values: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = values
        .into_iter()
        .map(|v| format!("\"{}\"", escape(&v)))
        .collect();
    format!("[{}]", items.join(","))
}

/// Which projects an `--app` scope admits, indexed by project.
///
/// Matching is against the project's include path, so `--app apps/business`
/// reaches every project under that app. A scope that admits nothing is a
/// mistyped pattern rather than an empty result, so it ends the run.
pub(crate) fn projects_in_scope(config: &Config, app: Option<&str>) -> Vec<bool> {
    let in_scope: Vec<bool> = config
        .projects()
        .iter()
        .map(|project| match app {
            Some(needle) => project.include().as_key().contains(needle),
            None => true,
        })
        .collect();

    if let Some(needle) = app
        && !in_scope.iter().any(|scoped| *scoped)
    {
        eprintln!(
            "{}: no project matched --app '{}'. Known projects:",
            "Error".red(),
            needle
        );
        for project in config.projects() {
            eprintln!("  {}", project.include().as_key());
        }
        graphox_core::utils::flush_stdio();
        std::process::exit(1);
    }

    in_scope
}

/// A project, its files, and the fragments it can see.
pub(crate) struct ResolvedProject {
    pub project_idx: usize,
    pub schema_key: String,
    /// The project's scanned files, as keys into `WorkspaceMetadata::documents`.
    pub files: Vec<PathBuf>,
    pub context: ProjectContext,
}

/// Resolve every in-scope project's fragments, as codegen does.
///
/// `selections` and `usage` read masked source grouped by schema, which is all
/// it takes to see what a definition selects in its own body. Following a spread
/// is a different question: which fragment a name resolves to depends on the
/// project doing the resolving, since a fragment reaches another project only
/// when it is `@public`, and two projects with overlapping `include` patterns
/// resolve the same name to their own copy. An analysis that expands spreads —
/// or that measures generated output, which is emitted per project — therefore
/// runs per project rather than per schema.
///
/// A project whose schema did not load is skipped with a warning, matching how
/// the other tools treat one.
pub(crate) fn resolve_projects(
    config: &Config,
    workspace: &WorkspaceMetadata,
    schemas: &AHashMap<String, Result<ValidSchema, String>>,
    in_scope: &[bool],
) -> Vec<ResolvedProject> {
    config
        .projects()
        .par_iter()
        .enumerate()
        .filter(|(idx, _)| in_scope[*idx])
        .filter_map(|(project_idx, project)| {
            let schema_key = project.schema().as_key();
            let Some(Ok(schema)) = schemas.get(&schema_key) else {
                eprintln!(
                    "{}: skipping project {} — schema {} did not load",
                    "Warning".yellow(),
                    project.include().as_key().blue(),
                    schema_key
                );
                return None;
            };

            let files = workspace.projects[project_idx].files.clone();
            match Engine::resolve_project_context(
                config,
                project_idx,
                schema,
                &workspace.fragments,
                &files,
            ) {
                Ok(context) => Some(ResolvedProject {
                    project_idx,
                    schema_key,
                    files,
                    context,
                }),
                Err(e) => {
                    eprintln!(
                        "{}: skipping project {} — {}",
                        "Warning".yellow(),
                        project.include().as_key().blue(),
                        e
                    );
                    None
                }
            }
        })
        .collect()
}
