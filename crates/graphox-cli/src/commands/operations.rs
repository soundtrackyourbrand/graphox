use colored::*;
use graphox_core::Config;
use graphox_core::engine::Engine;
use graphox_features::analysis::DocumentSource;
use graphox_features::analysis::operations::{self, OperationCost, OperationKind};

use super::{
    build_validated_schemas, escape, json_strings, plural, projects_in_scope, resolve_projects,
    take,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Depth,
    Fields,
    Lists,
}

impl Sort {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "depth" => Some(Sort::Depth),
            "fields" => Some(Sort::Fields),
            "lists" => Some(Sort::Lists),
            _ => None,
        }
    }

    fn key(self, cost: &OperationCost) -> usize {
        match self {
            Sort::Depth => cost.depth,
            Sort::Fields => cost.fields,
            Sort::Lists => cost.list_nesting,
        }
    }
}

pub struct OperationsParams {
    /// Restrict to projects whose include path contains this.
    pub app: Option<String>,
    /// Report only queries, mutations or subscriptions.
    pub kind: Option<OperationKind>,
    /// Explain one operation rather than ranking all of them.
    pub name: Option<String>,
    pub sort: Sort,
    /// Rows to print, or zero for all. Does not apply to `--json`.
    pub limit: usize,
    pub json: bool,
}

pub async fn run_operations(config: Config, params: OperationsParams) {
    let workspace = Engine::scan_workspace(
        &config,
        tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
        None,
    );
    let schemas = build_validated_schemas(&config);
    let in_scope = projects_in_scope(&config, params.app.as_deref());

    let mut costs: Vec<OperationCost> = Vec::new();
    let mut unparsed = Vec::new();

    // Per project rather than per schema: a spread resolves through the
    // fragments its own project can see.
    for resolved in resolve_projects(&config, &workspace, &schemas, &in_scope) {
        let Some(Ok(schema)) = schemas.get(&resolved.schema_key) else {
            continue;
        };
        let sources: Vec<DocumentSource<'_>> = resolved
            .files
            .iter()
            .filter_map(|path| {
                workspace.documents.get(path).map(|doc| DocumentSource {
                    path: path.as_path(),
                    project_idx: resolved.project_idx,
                    source: doc.masked_source.as_ref(),
                })
            })
            .collect();
        if sources.is_empty() {
            continue;
        }

        let analysis = operations::analyze(schema, &sources, &resolved.context.all_fragments);
        costs.extend(analysis.operations);
        unparsed.extend(analysis.unparsed);
    }

    let matched: Vec<OperationCost> = costs
        .into_iter()
        .filter(|cost| params.kind.is_none_or(|kind| cost.kind == kind))
        .filter(|cost| params.name.as_ref().is_none_or(|name| &cost.name == name))
        .collect();

    if params.json {
        print_json(&config, &matched, &unparsed);
    } else {
        print_human(&config, matched, &unparsed, &params);
    }
}

fn project_label(config: &Config, project_idx: usize) -> String {
    config
        .projects()
        .get(project_idx)
        .map(|project| project.include().as_key())
        .unwrap_or_default()
}

/// Heaviest first by the chosen key, then by name so runs are comparable.
fn sorted(mut costs: Vec<OperationCost>, sort: Sort) -> Vec<OperationCost> {
    costs.sort_by(|a, b| {
        sort.key(b)
            .cmp(&sort.key(a))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    costs
}

fn print_human(
    config: &Config,
    matched: Vec<OperationCost>,
    unparsed: &[std::path::PathBuf],
    params: &OperationsParams,
) {
    if matched.is_empty() {
        println!("No operation matched.");
        return;
    }

    if !unparsed.is_empty() {
        println!(
            "{}",
            format!(
                "{} did not parse, so they contributed no operations",
                plural(unparsed.len(), "file")
            )
            .yellow()
        );
    }

    // Naming one operation means asking what makes it cost what it does.
    if params.name.is_some() {
        for cost in sorted(matched, params.sort) {
            println!(
                "\n{} {} {}",
                cost.name.bold(),
                cost.kind.as_str().bright_black(),
                config.relativize(&cost.path).display().to_string().blue()
            );
            if cost.partial {
                println!(
                    "{}",
                    "  a spread did not resolve, so these are lower bounds".yellow()
                );
            }
            let own = if cost.own_depth == cost.depth {
                String::new()
            } else {
                format!(" ({} in its own body)", cost.own_depth)
            };
            println!("  depth         {:>5}{}", cost.depth, own);
            println!("  fields        {:>5}", cost.fields);
            println!(
                "  list nesting  {:>5}{}",
                cost.list_nesting,
                if cost.list_path.is_empty() {
                    String::new()
                } else {
                    format!("  {}", cost.list_path)
                }
            );
            println!("  root fields   {:>5}", cost.root_fields);
            println!("  variables     {:>5}", cost.variables);
            println!("  spreads       {:>5}", cost.spreads);
            println!("  deepest path        {}", cost.deepest_path.bright_black());
        }
        return;
    }

    let total = matched.len();
    let deepest = matched.iter().map(|c| c.depth).max().unwrap_or(0);
    let multiplying = matched.iter().filter(|c| c.list_nesting >= 3).count();
    println!(
        "{}",
        format!(
            "{} in scope, deepest {}, {} nesting three or more lists",
            plural(total, "operation"),
            deepest,
            multiplying
        )
        .bright_black()
    );

    println!(
        "{:>5}  {:>4}  {:>6}  {:>5}  {}",
        "DEPTH".bright_black(),
        "OWN".bright_black(),
        "FIELDS".bright_black(),
        "LISTS".bright_black(),
        "OPERATION".bright_black()
    );

    let rows = take(sorted(matched, params.sort), params.limit);
    for cost in &rows {
        let lists = if cost.list_nesting >= 3 {
            cost.list_nesting.to_string().yellow()
        } else {
            cost.list_nesting.to_string().normal()
        };
        println!(
            "{:>5}  {:>4}  {:>6}  {:>5}  {}{}  {}",
            cost.depth,
            cost.own_depth,
            cost.fields,
            lists,
            cost.name,
            if cost.partial { "*" } else { "" },
            project_label(config, cost.project_idx).bright_black(),
        );
    }

    if rows.iter().any(|cost| cost.partial) {
        println!(
            "{}",
            "* a spread did not resolve, so that row is a lower bound".bright_black()
        );
    }
}

fn print_json(config: &Config, matched: &[OperationCost], unparsed: &[std::path::PathBuf]) {
    // Every matching operation, never truncated: `--limit` shapes the terminal
    // view, and cutting machine output to it would drop rows nobody asked to
    // lose.
    let entries: Vec<String> = matched
        .iter()
        .map(|cost| {
            format!(
                r#"{{"operation":"{}","kind":"{}","path":"{}","project":"{}","depth":{},"own_depth":{},"fields":{},"list_nesting":{},"list_path":"{}","deepest_path":"{}","root_fields":{},"variables":{},"spreads":{},"partial":{}}}"#,
                escape(&cost.name),
                cost.kind.as_str(),
                escape(&config.relativize(&cost.path).display().to_string()),
                escape(&project_label(config, cost.project_idx)),
                cost.depth,
                cost.own_depth,
                cost.fields,
                cost.list_nesting,
                escape(&cost.list_path),
                escape(&cost.deepest_path),
                cost.root_fields,
                cost.variables,
                cost.spreads,
                cost.partial,
            )
        })
        .collect();

    println!(
        "{{\"operations\":[{}],\"unparsed\":{}}}",
        entries.join(","),
        json_strings(
            unparsed
                .iter()
                .map(|p| config.relativize(p).display().to_string())
        )
    );
}
