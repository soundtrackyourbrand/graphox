use colored::*;
use graphox_core::Config;
use graphox_core::apollo_ast::{get_operation_fragment_dependencies, serialize_operation};
use graphox_core::apollo_compiler::ExecutableDocument;
use graphox_core::ast_printer::print_document;
use graphox_core::engine::Engine;
use std::path::PathBuf;
use std::sync::Arc;

use super::{PathScope, build_validated_schemas, escape, resolve_projects};

pub struct ExpandParams {
    pub operation: String,
    /// A project directory, or a file whose project to resolve the operation
    /// in. Without one, every project is searched.
    pub scope: Option<PathScope>,
    /// Print the request body a client posts instead of the document alone.
    pub json: bool,
}

/// One place the operation is defined, as the project there resolves it.
struct Expansion {
    path: PathBuf,
    project_idx: usize,
    document: String,
}

pub async fn run_expand(config: Config, params: ExpandParams) {
    let workspace = Engine::scan_workspace(
        &config,
        tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
        None,
    );

    // A file names the projects that include it and a directory the projects
    // with files under it; either way the operation is looked up in the whole
    // project, since a file's operation may live next to it.
    let in_scope: Vec<bool> = workspace
        .projects
        .iter()
        .map(|project| match &params.scope {
            Some(scope) => project.files.iter().any(|f| scope.contains(&config, f)),
            None => true,
        })
        .collect();
    if let Some(scope) = &params.scope
        && !in_scope.iter().any(|scoped| *scoped)
    {
        fail(&format!(
            "no project includes files under '{}'",
            scope.given()
        ));
    }

    let schemas = build_validated_schemas(&config);
    let mut expansions = Vec::new();
    let mut unresolved: Vec<(PathBuf, Vec<Arc<str>>)> = Vec::new();

    for resolved in resolve_projects(&config, &workspace, &schemas, &in_scope) {
        let Some(Ok(schema)) = schemas.get(&resolved.schema_key) else {
            continue;
        };
        let codegen_config = config.get_codegen_config(config.projects().get(resolved.project_idx));

        for path in &resolved.files {
            let Some(doc) = workspace.documents.get(path) else {
                continue;
            };
            let source: &str = doc.masked_source.as_ref();
            // Parsing every file to find one name is most of the run otherwise.
            if !source.contains(params.operation.as_str()) {
                continue;
            }
            let parsed = match ExecutableDocument::parse(schema, source, path) {
                Ok(parsed) => parsed,
                Err(with_errors) => with_errors.partial,
            };
            let Some(operation) = parsed.operations.named.get(params.operation.as_str()) else {
                continue;
            };

            // Spreads resolve through the project's fragments alone, as they do
            // in codegen, so the document is the one codegen emits.
            let fragments = &resolved.context.all_fragments;
            let mut missing: Vec<Arc<str>> =
                get_operation_fragment_dependencies(operation, fragments)
                    .into_iter()
                    .filter(|name| !fragments.contains_key(name))
                    .collect();
            if !missing.is_empty() {
                missing.sort_unstable();
                unresolved.push((path.clone(), missing));
                continue;
            }

            let ast = serialize_operation(operation, fragments, &codegen_config);
            expansions.push(Expansion {
                path: path.clone(),
                project_idx: resolved.project_idx,
                document: print_document(&ast),
            });
        }
    }

    // A document missing a fragment is not one any server accepts, so printing
    // what did resolve would hand over a request that fails.
    if !unresolved.is_empty() {
        for (path, missing) in &unresolved {
            eprintln!(
                "{}: {} in {} spreads fragments that did not resolve: {}",
                "Error".red(),
                params.operation.bold(),
                config.relativize(path).display(),
                missing.join(", ")
            );
        }
        graphox_core::utils::flush_stdio();
        std::process::exit(1);
    }

    let Some(first) = expansions.first() else {
        let place = match &params.scope {
            Some(scope) => format!("the projects under '{}'", scope.given()),
            None => "the workspace".to_string(),
        };
        fail(&format!(
            "no operation named '{}' in {place}",
            params.operation
        ));
    };

    // Overlapping includes put one file in several projects, which is only a
    // question when they resolve it differently.
    if expansions.iter().any(|e| e.document != first.document) {
        eprintln!(
            "{}: '{}' expands differently in {} places. Pass a path to choose one:",
            "Error".red(),
            params.operation,
            expansions.len()
        );
        for expansion in &expansions {
            eprintln!(
                "  {}  {}",
                config.relativize(&expansion.path).display(),
                config
                    .projects()
                    .get(expansion.project_idx)
                    .map(|project| project.include().as_key())
                    .unwrap_or_default()
                    .bright_black()
            );
        }
        graphox_core::utils::flush_stdio();
        std::process::exit(1);
    }

    if params.json {
        println!(
            r#"{{"operationName":"{}","query":"{}"}}"#,
            escape(&params.operation),
            escape(&first.document)
        );
    } else {
        println!("{}", first.document);
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{}: {message}", "Error".red());
    graphox_core::utils::flush_stdio();
    std::process::exit(1);
}
