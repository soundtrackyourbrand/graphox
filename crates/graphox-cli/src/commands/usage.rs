use colored::*;
use graphox_core::Config;
use graphox_core::engine::Engine;
use graphox_features::analysis::DocumentSource;
use graphox_features::analysis::usage::{self, FieldUsage, Usage};

use super::{build_validated_schemas, documents_by_schema};

pub struct UsageParams {
    /// Restrict to projects whose include path contains this, so `--app
    /// business` reaches every project under that app.
    pub app: Option<String>,
    /// Restrict to one GraphQL type.
    pub type_name: Option<String>,
    /// Restrict to one field name.
    pub field: Option<String>,
    /// Report only what nothing selects.
    pub unused: bool,
    /// Rows to print, or zero for all.
    pub limit: usize,
    pub json: bool,
}

/// A row after scoping: the consumers that survived the `--app` filter.
struct Row<'a> {
    field: &'a FieldUsage,
    consumers: Vec<usize>,
}

impl Row<'_> {
    fn is_unused(&self) -> bool {
        self.consumers.is_empty()
    }
}

pub async fn run_usage(config: Config, params: UsageParams) {
    let workspace = Engine::scan_workspace(
        &config,
        tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
        None,
    );
    let schemas = build_validated_schemas(&config);

    // Which projects the `--app` scope admits. A definition outside them is not
    // a consumer for this run, so counts read as "usage within that app".
    let in_scope: Vec<bool> = config
        .projects()
        .iter()
        .map(|project| match &params.app {
            Some(needle) => project.include().as_key().contains(needle.as_str()),
            None => true,
        })
        .collect();

    if params.app.is_some() && !in_scope.iter().any(|scoped| *scoped) {
        eprintln!(
            "{}: no project matched --app '{}'. Known projects:",
            "Error".red(),
            params.app.as_deref().unwrap_or_default()
        );
        for project in config.projects() {
            eprintln!("  {}", project.include().as_key());
        }
        graphox_core::utils::flush_stdio();
        std::process::exit(1);
    }

    let mut analyses: Vec<(String, Usage)> = Vec::new();
    for (schema_key, files) in documents_by_schema(&config, &workspace) {
        let Some(Ok(schema)) = schemas.get(&schema_key) else {
            eprintln!(
                "{}: skipping schema {} — it did not load",
                "Warning".yellow(),
                schema_key
            );
            continue;
        };
        let sources: Vec<DocumentSource<'_>> = files
            .iter()
            .filter(|(project_idx, _)| in_scope[*project_idx])
            .filter_map(|(project_idx, path)| {
                workspace.documents.get(path).map(|doc| DocumentSource {
                    path: path.as_path(),
                    project_idx: *project_idx,
                    source: doc.masked_source.as_ref(),
                })
            })
            .collect();
        if sources.is_empty() {
            continue;
        }
        analyses.push((schema_key, usage::analyze(schema, &sources)));
    }

    if params.json {
        print_json(&config, &analyses, &params);
    } else {
        print_human(&config, &analyses, &params);
    }
}

/// Fields matching the scope, with their surviving consumers.
fn rows<'a>(analysis: &'a Usage, params: &UsageParams) -> Vec<Row<'a>> {
    analysis
        .fields
        .iter()
        .filter(|field| {
            params
                .type_name
                .as_ref()
                .is_none_or(|t| &field.type_name == t)
                && params.field.as_ref().is_none_or(|f| &field.field_name == f)
        })
        .map(|field| Row {
            field,
            consumers: field.consumers.clone(),
        })
        .filter(|row| !params.unused || row.is_unused())
        .collect()
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn take<T>(items: Vec<T>, limit: usize) -> Vec<T> {
    if limit == 0 {
        items
    } else {
        items.into_iter().take(limit).collect()
    }
}

fn print_human(config: &Config, analyses: &[(String, Usage)], params: &UsageParams) {
    for (schema_key, analysis) in analyses {
        // A named field or type belongs to one schema; skip the schemas that do
        // not have it rather than printing an empty heading for each.
        let scoped = rows(analysis, params);
        let named = params.type_name.is_some() || params.field.is_some();
        if named && scoped.is_empty() {
            continue;
        }
        // Several schemas can declare the same type while only one selects it.
        // A block of nothing but zeroes is noise, unless zeroes are the point.
        if named && !params.unused && scoped.iter().all(|row| row.is_unused()) {
            continue;
        }

        println!("\n{} {}", "Schema".bright_black(), schema_key.blue());
        if !analysis.unparsed.is_empty() {
            println!(
                "{}",
                format!(
                    "{} did not parse against this schema, so these counts are low",
                    plural(analysis.unparsed.len(), "file")
                )
                .yellow()
            );
        }

        // Naming a single field means asking who uses it, so list them.
        if params.field.is_some() {
            for row in take(scoped, params.limit) {
                println!(
                    "\n{} {}",
                    format!("{}.{}", row.field.type_name, row.field.field_name).bold(),
                    if row.is_unused() {
                        "unused".yellow()
                    } else {
                        plural(row.consumers.len(), "consumer").bright_black()
                    }
                );
                for id in &row.consumers {
                    let definition = &analysis.definitions[*id];
                    println!(
                        "  {}  {}",
                        definition.name,
                        config
                            .relativize(&definition.path)
                            .display()
                            .to_string()
                            .blue()
                    );
                }
            }
            continue;
        }

        // A named type means asking which of its fields are pulling weight.
        if let Some(type_name) = &params.type_name {
            // Under --unused the rows are already only the unused ones, so
            // "0 of N used" would describe the filter rather than the type.
            let heading = if params.unused {
                format!(
                    "{type_name}: {} nothing selects",
                    plural(scoped.len(), "declared field")
                )
            } else {
                let used = scoped.iter().filter(|r| !r.is_unused()).count();
                format!("{type_name}: {used} of {} fields used", scoped.len())
            };
            println!("{}", heading.bright_black());
            let mut sorted = scoped;
            sorted.sort_by(|a, b| {
                b.consumers
                    .len()
                    .cmp(&a.consumers.len())
                    .then_with(|| a.field.field_name.cmp(&b.field.field_name))
            });
            for row in take(sorted, params.limit) {
                let count = row.consumers.len();
                let rendered = format!("{count:>4}  {}", row.field.field_name);
                println!(
                    "  {}",
                    if count == 0 {
                        rendered.yellow()
                    } else {
                        rendered.normal()
                    }
                );
            }
            continue;
        }

        // Otherwise: which types the workspace leans on, and how much of each
        // one it actually selects.
        if params.unused {
            let mut unused = scoped;
            unused.sort_by(|a, b| {
                a.field
                    .type_name
                    .cmp(&b.field.type_name)
                    .then_with(|| a.field.field_name.cmp(&b.field.field_name))
            });
            println!(
                "{}",
                format!("{} nothing selects", plural(unused.len(), "declared field"))
                    .bright_black()
            );
            for row in take(unused, params.limit) {
                println!(
                    "  {}.{}",
                    row.field.type_name.bright_black(),
                    row.field.field_name.yellow()
                );
            }
            continue;
        }

        let used_types = analysis
            .types
            .iter()
            .filter(|t| !t.consumers.is_empty())
            .count();
        println!(
            "{}",
            format!(
                "{} of {} declared types used by {} definitions",
                used_types,
                analysis.types.len(),
                analysis.definitions.len()
            )
            .bright_black()
        );
        println!(
            "{:>6}  {:>7}  {}",
            "USERS".bright_black(),
            "FIELDS".bright_black(),
            "TYPE".bright_black()
        );
        for ty in take(analysis.types.iter().collect(), params.limit) {
            if ty.consumers.is_empty() {
                continue;
            }
            let used = ty.declared_fields - ty.unused_fields;
            println!(
                "{:>6}  {:>7}  {}",
                ty.consumers.len(),
                format!("{}/{}", used, ty.declared_fields),
                ty.name
            );
        }
    }
}

fn escape(value: &str) -> String {
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

fn json_strings(values: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = values
        .into_iter()
        .map(|v| format!("\"{}\"", escape(&v)))
        .collect();
    format!("[{}]", items.join(","))
}

fn print_json(config: &Config, analyses: &[(String, Usage)], params: &UsageParams) {
    let mut entries: Vec<String> = Vec::new();
    let mut unparsed: Vec<String> = Vec::new();

    for (schema_key, analysis) in analyses {
        unparsed.extend(
            analysis
                .unparsed
                .iter()
                .map(|p| config.relativize(p).display().to_string()),
        );

        // Every matching field, never truncated: `--limit` shapes the terminal
        // view, and cutting machine output to it would drop rows nobody asked
        // to lose.
        for row in rows(analysis, params) {
            let consumers = row.consumers.iter().map(|id| {
                let definition = &analysis.definitions[*id];
                format!(
                    r#"{{"definition":"{}","path":"{}"}}"#,
                    escape(&definition.name),
                    escape(&config.relativize(&definition.path).display().to_string()),
                )
            });
            entries.push(format!(
                r#"{{"schema":"{}","type":"{}","field":"{}","consumers":{},"consumer_count":{}}}"#,
                escape(schema_key),
                escape(&row.field.type_name),
                escape(&row.field.field_name),
                format_args!("[{}]", consumers.collect::<Vec<_>>().join(",")),
                row.consumers.len(),
            ));
        }
    }

    println!(
        "{{\"fields\":[{}],\"unparsed\":{}}}",
        entries.join(","),
        json_strings(unparsed)
    );
}
