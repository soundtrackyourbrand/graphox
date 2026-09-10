use colored::*;
use graphox_core::Config;
use graphox_core::engine::Engine;
use graphox_features::analysis::repeated_selections::{self, Analysis, OverlapKind, Scope};
use graphox_features::analysis::{DefinitionKind, DocumentSource};

use super::{
    build_validated_schemas, documents_by_schema, escape, json_strings, mandated_fields_by_project,
    plural, take,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Matches,
    Extends,
    New,
}

impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "matches_fragment" => Some(Kind::Matches),
            "extends_fragment" => Some(Kind::Extends),
            "new_fragment" => Some(Kind::New),
            _ => None,
        }
    }
}

pub struct AnalyzeParams {
    pub min_fields: usize,
    pub min_uses: usize,
    /// Restrict output to one kind of finding. `None` reports all three.
    pub kind: Option<Kind>,
    /// Restrict output to one GraphQL type.
    pub type_name: Option<String>,
    /// Findings to print per section in the human view. Zero means all of
    /// them. JSON ignores it: the default exists to keep a terminal readable,
    /// and silently truncating machine output to it would lose findings a
    /// consumer never asked to drop.
    pub limit: usize,
    pub json: bool,
}

pub async fn run_analyze(config: Config, params: AnalyzeParams) {
    let workspace = Engine::scan_workspace(
        &config,
        tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
        None,
    );
    let schemas = build_validated_schemas(&config);

    let by_schema = documents_by_schema(&config, &workspace);

    let options = repeated_selections::Options {
        min_fields: params.min_fields,
        min_uses: params.min_uses,
        mandated_by_project: mandated_fields_by_project(&config),
    };

    let mut analyses = Vec::new();
    for (schema_key, files) in &by_schema {
        let Some(Ok(schema)) = schemas.get(schema_key) else {
            eprintln!(
                "{}: skipping schema {} — it did not load",
                "Warning".yellow(),
                schema_key
            );
            continue;
        };

        let sources: Vec<DocumentSource<'_>> = files
            .iter()
            .filter_map(|(project_idx, path)| {
                workspace.documents.get(path).map(|doc| DocumentSource {
                    path: path.as_path(),
                    project_idx: *project_idx,
                    source: doc.masked_source.as_ref(),
                })
            })
            .collect();

        analyses.push((
            schema_key.clone(),
            repeated_selections::analyze(schema, &sources, &options),
        ));
    }

    if params.json {
        print_json(&config, &analyses, &params);
    } else {
        print_human(&config, &analyses, &params);
    }
}

fn wanted(params: &AnalyzeParams, kind: Kind, type_name: &str) -> bool {
    if params.kind.is_some_and(|k| k != kind) {
        return false;
    }
    if let Some(filter) = &params.type_name
        && filter != type_name
    {
        return false;
    }
    true
}

fn print_human(config: &Config, analyses: &[(String, Analysis)], params: &AnalyzeParams) {
    for (schema_key, analysis) in analyses {
        let operations = analysis
            .definitions
            .iter()
            .filter(|d| d.kind == DefinitionKind::Operation)
            .count();
        let fragments = analysis.definitions.len() - operations;

        let has_overlaps = analysis.overlaps.iter().any(|o| {
            let kind = match o.kind {
                OverlapKind::Matches => Kind::Matches,
                OverlapKind::Extends => Kind::Extends,
            };
            wanted(params, kind, &o.type_name)
        });
        let has_groups = analysis
            .groups
            .iter()
            .any(|g| wanted(params, Kind::New, &g.type_name));
        if !has_overlaps && !has_groups {
            continue;
        }

        println!("\n{} {}", "Schema".bright_black(), schema_key.blue());
        println!(
            "{}",
            format!("{operations} operations, {fragments} fragments").bright_black()
        );

        for (kind, heading) in [
            (Kind::Matches, "Duplicates a fragment that already exists"),
            (Kind::Extends, "Selects a fragment's fields, plus more"),
        ] {
            let overlap_kind = if kind == Kind::Matches {
                OverlapKind::Matches
            } else {
                OverlapKind::Extends
            };
            let rows: Vec<_> = analysis
                .overlaps
                .iter()
                .filter(|o| o.kind == overlap_kind && wanted(params, kind, &o.type_name))
                .collect();
            if rows.is_empty() {
                continue;
            }

            let total = rows.len();
            let shown = take(rows, params.limit);
            println!(
                "\n{} {}",
                heading.bold(),
                format!("({total})").bright_black()
            );
            for overlap in shown {
                let definition = &analysis.definitions[overlap.site.definition];
                println!(
                    "  {} {} {}",
                    definition.name.bold(),
                    "->".bright_black(),
                    format!("{} on {}", overlap.fragment, overlap.type_name).yellow()
                );
                println!(
                    "    {}",
                    config
                        .relativize(&definition.path)
                        .display()
                        .to_string()
                        .blue()
                );
                if !overlap.extra.is_empty() {
                    println!(
                        "    {} {}",
                        "plus".bright_black(),
                        overlap.extra.join(" ").bright_black()
                    );
                }
            }
        }

        let groups: Vec<_> = analysis
            .groups
            .iter()
            .filter(|g| g.covered_by.is_none() && wanted(params, Kind::New, &g.type_name))
            .collect();
        if !groups.is_empty() {
            // Overlapping shapes on one type (`{id name}`, `{id name kind}`)
            // all describe the same duplication. Lead with the widest-reaching
            // shape per type and count the rest, unless a type was asked for.
            let mut leading: Vec<&&repeated_selections::Group> = Vec::new();
            let mut variants: Vec<usize> = Vec::new();
            for group in &groups {
                match leading
                    .iter()
                    .position(|kept| kept.type_name == group.type_name)
                {
                    Some(idx) if params.type_name.is_none() => variants[idx] += 1,
                    _ => {
                        leading.push(group);
                        variants.push(0);
                    }
                }
            }

            println!(
                "\n{} {}",
                "Could become a fragment".bold(),
                format!("({})", plural(groups.len(), "shape")).bright_black()
            );
            let limit = if params.limit == 0 {
                leading.len()
            } else {
                params.limit.min(leading.len())
            };
            for (group, others) in leading.into_iter().zip(variants).take(limit) {
                let scope = match group.scope {
                    Scope::InProject => "in-project".green(),
                    Scope::CrossProject => "cross-project".yellow(),
                };
                let related = if others > 0 {
                    format!(" +{others} related shapes")
                } else {
                    String::new()
                };
                println!(
                    "  {} {} {}{}",
                    format!("on {}", group.type_name).bold(),
                    plural(group.definitions.len(), "definition").bright_black(),
                    scope,
                    related.bright_black()
                );
                println!("    {{ {} }}", group.members.join(" "));
                let names: Vec<String> = group
                    .definitions
                    .iter()
                    .take(6)
                    .map(|id| analysis.definitions[*id].name.clone())
                    .collect();
                let more = group.definitions.len().saturating_sub(names.len());
                let suffix = if more > 0 {
                    format!(" +{more}")
                } else {
                    String::new()
                };
                println!(
                    "    {}",
                    format!("{}{}", names.join(", "), suffix).bright_black()
                );
            }
        }

        if !analysis.unparsed.is_empty() {
            println!(
                "\n{} {} file(s) had GraphQL that did not parse against this schema",
                "Note:".bright_black(),
                analysis.unparsed.len()
            );
        }
    }
}

fn print_json(config: &Config, analyses: &[(String, Analysis)], params: &AnalyzeParams) {
    let mut findings: Vec<String> = Vec::new();

    for (schema_key, analysis) in analyses {
        for overlap in &analysis.overlaps {
            let kind = match overlap.kind {
                OverlapKind::Matches => Kind::Matches,
                OverlapKind::Extends => Kind::Extends,
            };
            if !wanted(params, kind, &overlap.type_name) {
                continue;
            }
            let definition = &analysis.definitions[overlap.site.definition];
            let kind_name = match overlap.kind {
                OverlapKind::Matches => "matches_fragment",
                OverlapKind::Extends => "extends_fragment",
            };
            findings.push(format!(
                r#"{{"kind":"{}","schema":"{}","type":"{}","fragment":"{}","definition":"{}","path":"{}","extra":{}}}"#,
                kind_name,
                escape(schema_key),
                escape(&overlap.type_name),
                escape(&overlap.fragment),
                escape(&definition.name),
                escape(&config.relativize(&definition.path).display().to_string()),
                json_strings(overlap.extra.clone()),
            ));
        }

        for group in &analysis.groups {
            if group.covered_by.is_some() || !wanted(params, Kind::New, &group.type_name) {
                continue;
            }
            let names = group
                .definitions
                .iter()
                .map(|id| analysis.definitions[*id].name.clone());
            let paths = group.definitions.iter().map(|id| {
                config
                    .relativize(&analysis.definitions[*id].path)
                    .display()
                    .to_string()
            });
            findings.push(format!(
                r#"{{"kind":"new_fragment","schema":"{}","type":"{}","scope":"{}","members":{},"definitions":{},"paths":{}}}"#,
                escape(schema_key),
                escape(&group.type_name),
                match group.scope {
                    Scope::InProject => "in_project",
                    Scope::CrossProject => "cross_project",
                },
                json_strings(group.members.clone()),
                json_strings(names),
                json_strings(paths),
            ));
        }
    }

    // Files whose GraphQL did not parse are why a finding might be missing, so
    // a consumer needs them as much as the findings themselves.
    let unparsed: Vec<String> = analyses
        .iter()
        .flat_map(|(_, analysis)| &analysis.unparsed)
        .map(|path| config.relativize(path).display().to_string())
        .collect();

    println!(
        "{{\"findings\":[{}],\"unparsed\":{}}}",
        findings.join(","),
        json_strings(unparsed)
    );
}
