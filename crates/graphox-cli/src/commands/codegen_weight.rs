//! `analyze codegen` — what the generated TypeScript weighs, and which
//! definition it came from.
//!
//! It generates the same output `codegen` writes and measures it instead of
//! writing it, so the numbers describe the real files rather than an estimate
//! of them.

use colored::*;
use graphox_codegen as codegen;
use graphox_core::Config;
use graphox_features::analysis::{DocumentSource, spreads};
use rayon::prelude::*;
use std::path::PathBuf;

use super::setup::{CodegenSetup, file_paths};
use super::{escape, json_strings, plural, projects_in_scope, resolve_projects, take};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Operation,
    Fragment,
}

impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "operation" => Some(Kind::Operation),
            "fragment" => Some(Kind::Fragment),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Kind::Operation => "operation",
            Kind::Fragment => "fragment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Bytes,
    Ast,
}

impl Sort {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bytes" => Some(Sort::Bytes),
            "ast" => Some(Sort::Ast),
            _ => None,
        }
    }

    fn key(self, row: &Row) -> usize {
        match self {
            Sort::Bytes => row.generated_bytes,
            Sort::Ast => row.ast_bytes,
        }
    }
}

pub struct CodegenWeightParams {
    /// Restrict to projects whose include path contains this.
    pub app: Option<String>,
    /// Report only operations, or only fragments.
    pub kind: Option<Kind>,
    pub sort: Sort,
    /// Rows to print, or zero for all. Does not apply to `--json`.
    pub limit: usize,
    pub json: bool,
}

/// What one definition contributed to its project's generated output.
struct Row {
    name: String,
    kind: Kind,
    source: PathBuf,
    project_idx: usize,
    generated_bytes: usize,
    ast_bytes: usize,
    /// Definitions that spread this fragment, so its weight can be read as
    /// leverage. `None` for an operation, which nothing spreads.
    spread_by: Option<usize>,
}

#[derive(Default)]
struct ProjectWeight {
    project_idx: usize,
    rows: Vec<Row>,
    /// Output bytes no definition accounts for: the per-file preamble, the
    /// helper types and the import lines. Real files, so this is measured
    /// rather than assumed.
    shared_bytes: usize,
    files: usize,
    /// Files whose GraphQL the generator rejected. They contribute no rows, so
    /// their absence would otherwise read as a project with less output.
    failed: Vec<PathBuf>,
}

pub async fn run_codegen_weight(config: Config, params: CodegenWeightParams) {
    let setup = CodegenSetup::resolve(&config);
    let in_scope = projects_in_scope(&config, params.app.as_deref());
    let caches = codegen::SchemaAnalysisCaches::new();

    let resolved = resolve_projects(&config, &setup.workspace, &setup.schemas, &in_scope);

    let mut weights: Vec<ProjectWeight> = resolved
        .par_iter()
        .filter_map(|project| {
            let settings = config.projects().get(project.project_idx)?;
            if !config.get_project_codegen_enabled(settings) {
                return None;
            }
            let Some(Ok(schema)) = setup.schemas.get(&project.schema_key) else {
                return None;
            };

            let codegen_config = config.get_codegen_config(Some(settings));
            let emit_extensions = config.get_emit_extensions(settings);
            let type_imports = setup.type_imports_for(&project.schema_key);
            let schema_import = setup.schema_import(&config, settings);
            let output_dir = settings.output_dir().map(std::path::Path::new);

            // Counted per project, since a fragment shared by two projects is
            // generated into each of them and weighs in both.
            let sources: Vec<DocumentSource<'_>> = project
                .files
                .iter()
                .filter_map(|path| {
                    setup
                        .workspace
                        .documents
                        .get(path)
                        .map(|doc| DocumentSource {
                            path: path.as_path(),
                            project_idx: project.project_idx,
                            source: doc.masked_source.as_ref(),
                        })
                })
                .collect();
            let spread_counts = spreads::direct_counts(schema, &sources);

            let mut weight = ProjectWeight {
                project_idx: project.project_idx,
                ..Default::default()
            };

            for path in &project.files {
                let Some(doc) = setup.workspace.documents.get(path) else {
                    continue;
                };
                if doc.get_graphql_trees().is_empty() {
                    continue;
                }

                let paths = file_paths(
                    config.base_dir(),
                    settings.include(),
                    output_dir,
                    emit_extensions,
                    path,
                );
                let ctx = codegen::CodegenContext::new(
                    schema,
                    &project.context.fragment_to_path,
                    &project.context.fragment_output_paths,
                    &project.context.fragment_to_import,
                    &project.context.fragment_to_type_only,
                    &project.context.all_fragments,
                    &project.context.name_to_id,
                    path,
                    config.scalars(),
                    &schema_import,
                    type_imports,
                    codegen_config.generate_ast_for_fragments(),
                    &project.context.fragment_dependencies,
                    &caches,
                    &codegen_config,
                    paths.masking_import_path,
                    paths.out_path,
                );

                let (output, operations, fragments) = match codegen::generate_typescript(doc, &ctx)
                {
                    Ok(generated) => generated,
                    // A file holding GraphQL that generates nothing is normal;
                    // one the generator rejected is worth saying out loud.
                    Err(e) if e.contains("No executable operations") => continue,
                    Err(_) => {
                        weight.failed.push(path.clone());
                        continue;
                    }
                };

                let mut attributed = 0;
                for operation in operations {
                    attributed += operation.generated_bytes;
                    weight.rows.push(Row {
                        name: operation.name,
                        kind: Kind::Operation,
                        source: path.clone(),
                        project_idx: project.project_idx,
                        generated_bytes: operation.generated_bytes,
                        ast_bytes: operation.ast_bytes,
                        spread_by: None,
                    });
                }
                for fragment in fragments {
                    attributed += fragment.generated_bytes;
                    let spread_by = spread_counts.get(&fragment.name).copied().unwrap_or(0);
                    weight.rows.push(Row {
                        name: fragment.name,
                        kind: Kind::Fragment,
                        source: path.clone(),
                        project_idx: project.project_idx,
                        generated_bytes: fragment.generated_bytes,
                        ast_bytes: fragment.ast_bytes,
                        spread_by: Some(spread_by),
                    });
                }

                weight.files += 1;
                weight.shared_bytes += output.len().saturating_sub(attributed);
            }

            Some(weight)
        })
        .collect();

    weights.sort_by_key(|weight| weight.project_idx);

    if params.json {
        print_json(&config, &weights, &params);
    } else {
        print_human(&config, &weights, &params);
    }
}

fn project_label(config: &Config, project_idx: usize) -> String {
    config
        .projects()
        .get(project_idx)
        .map(|project| project.include().as_key())
        .unwrap_or_default()
}

/// Rows the `--kind` filter admits, heaviest first, then by name so runs are
/// comparable.
fn matching<'a>(weights: &'a [ProjectWeight], params: &CodegenWeightParams) -> Vec<&'a Row> {
    let mut rows: Vec<&Row> = weights
        .iter()
        .flat_map(|weight| weight.rows.iter())
        .filter(|row| params.kind.is_none_or(|kind| row.kind == kind))
        .collect();
    rows.sort_by(|a, b| {
        params
            .sort
            .key(b)
            .cmp(&params.sort.key(a))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.source.cmp(&b.source))
    });
    rows
}

/// Bytes as KB once they stop being readable as bytes.
fn size(bytes: usize) -> String {
    if bytes < 10_000 {
        format!("{bytes}")
    } else {
        format!("{}K", bytes / 1000)
    }
}

fn print_human(config: &Config, weights: &[ProjectWeight], params: &CodegenWeightParams) {
    let failed: Vec<&PathBuf> = weights
        .iter()
        .flat_map(|weight| weight.failed.iter())
        .collect();
    if !failed.is_empty() {
        println!(
            "{}",
            format!(
                "{} did not generate, so their output is missing from these totals",
                plural(failed.len(), "file")
            )
            .yellow()
        );
    }

    let rows = matching(weights, params);
    if rows.is_empty() {
        println!("Nothing generated.");
        return;
    }

    let attributed: usize = rows.iter().map(|row| row.generated_bytes).sum();
    let ast: usize = rows.iter().map(|row| row.ast_bytes).sum();
    let shared: usize = weights.iter().map(|weight| weight.shared_bytes).sum();
    // Files holding a row that is in scope, not files generated: under a
    // `--kind` filter the latter would count files whose definitions were
    // mostly excluded.
    let mut sources: Vec<&PathBuf> = rows.iter().map(|row| &row.source).collect();
    sources.sort_unstable();
    sources.dedup();

    // Shared bytes describe whole files, so they only reconcile with the total
    // when every definition in them is in scope.
    let filtered = params.kind.is_some();
    println!(
        "{}",
        format!(
            "{} in {} across {}: {} of TypeScript, {} of it document ASTs{}",
            plural(rows.len(), "definition"),
            plural(sources.len(), "file"),
            plural(weights.len(), "project"),
            size(attributed),
            size(ast),
            if filtered {
                String::new()
            } else {
                format!(", plus {} of per-file preamble and imports", size(shared))
            }
        )
        .bright_black()
    );

    println!(
        "{:>8}  {:>8}  {:>7}  {}",
        "BYTES".bright_black(),
        "AST".bright_black(),
        "SPREADS".bright_black(),
        "DEFINITION".bright_black()
    );
    for row in take(rows, params.limit) {
        println!(
            "{:>8}  {:>8}  {:>7}  {} {}  {}",
            size(row.generated_bytes),
            size(row.ast_bytes),
            row.spread_by
                .map(|count| count.to_string())
                .unwrap_or_else(|| "-".to_string()),
            row.name,
            row.kind.as_str().bright_black(),
            project_label(config, row.project_idx).bright_black(),
        );
    }

    if !filtered {
        println!(
            "\n{:>8}  {:>8}  {}",
            "BYTES".bright_black(),
            "AST".bright_black(),
            "PROJECT".bright_black()
        );
        let mut by_project: Vec<&ProjectWeight> = weights.iter().collect();
        by_project.sort_by_key(|weight| {
            std::cmp::Reverse(
                weight
                    .rows
                    .iter()
                    .map(|row| row.generated_bytes)
                    .sum::<usize>()
                    + weight.shared_bytes,
            )
        });
        for weight in by_project {
            let total: usize = weight
                .rows
                .iter()
                .map(|row| row.generated_bytes)
                .sum::<usize>()
                + weight.shared_bytes;
            let ast: usize = weight.rows.iter().map(|row| row.ast_bytes).sum();
            println!(
                "{:>8}  {:>8}  {}",
                size(total),
                size(ast),
                project_label(config, weight.project_idx)
            );
        }
    }
}

fn print_json(config: &Config, weights: &[ProjectWeight], params: &CodegenWeightParams) {
    // Every matching definition, never truncated: `--limit` shapes the terminal
    // view, and cutting machine output to it would drop rows nobody asked to
    // lose.
    let entries: Vec<String> = matching(weights, params)
        .iter()
        .map(|row| {
            format!(
                r#"{{"definition":"{}","kind":"{}","path":"{}","project":"{}","generated_bytes":{},"ast_bytes":{},"spread_by":{}}}"#,
                escape(&row.name),
                row.kind.as_str(),
                escape(&config.relativize(&row.source).display().to_string()),
                escape(&project_label(config, row.project_idx)),
                row.generated_bytes,
                row.ast_bytes,
                row.spread_by
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "null".to_string()),
            )
        })
        .collect();

    let projects: Vec<String> = weights
        .iter()
        .map(|weight| {
            format!(
                r#"{{"project":"{}","files":{},"generated_bytes":{},"ast_bytes":{},"shared_bytes":{}}}"#,
                escape(&project_label(config, weight.project_idx)),
                weight.files,
                weight
                    .rows
                    .iter()
                    .map(|row| row.generated_bytes)
                    .sum::<usize>(),
                weight.rows.iter().map(|row| row.ast_bytes).sum::<usize>(),
                weight.shared_bytes,
            )
        })
        .collect();

    println!(
        "{{\"definitions\":[{}],\"projects\":[{}],\"failed\":{}}}",
        entries.join(","),
        projects.join(","),
        json_strings(
            weights
                .iter()
                .flat_map(|weight| weight.failed.iter())
                .map(|p| config.relativize(p).display().to_string())
        )
    );
}
