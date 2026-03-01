use criterion::{Criterion, criterion_group, criterion_main};
use graphox::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
    document::DocumentState,
};
use std::fs;
use std::path::Path;
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

// SLO classes available in the system
const SLO_CLASSES: &[&str] = &["CRITICAL", "HIGH_FAST", "HIGH_SLOW", "LOW"];

/// Generate a workspace with subgraphs and SLO directives
/// Matches the "500 files, 10 projects, 50 types × 20 fields" setup
/// but adds 10 subgraphs per project with SLO directives on 50% of fields
#[allow(clippy::too_many_arguments)]
fn generate_subgraph_workspace(
    base_dir: &Path,
    projects_count: usize,
    files_per_project: usize,
    type_count: usize,
    fields_per_type: usize,
    subgraphs_per_project: usize,
    slo_field_ratio: f64,
    seed: u64,
) -> Config {
    let mut projects = Vec::new();

    // Seeded random for consistent results
    let mut rng_state = seed;

    let next_rand = |state: &mut u64| -> u64 {
        *state = state.wrapping_mul(1103515245).wrapping_add(12345);
        *state
    };

    for i in 0..projects_count {
        let project_dir = base_dir.join(format!("project_{}", i));
        fs::create_dir_all(&project_dir).unwrap();

        // Create subgraphs directory
        let subgraphs_dir = project_dir.join("subgraphs");
        fs::create_dir_all(&subgraphs_dir).unwrap();

        // Pre-generate subgraph classes for this project
        let mut subgraph_classes: Vec<&str> = Vec::new();
        for _ in 0..subgraphs_per_project {
            let slo_class = SLO_CLASSES[(next_rand(&mut rng_state) as usize) % SLO_CLASSES.len()];
            subgraph_classes.push(slo_class);
        }

        // Generate subgraphs with extend type and SLO directives
        for (s, &slo_class) in subgraph_classes.iter().enumerate() {
            let subgraph_path = subgraphs_dir.join(format!("subgraph_{}.graphql", s));
            let mut subgraph_content = String::new();

            // Schema-level SLO directive
            subgraph_content.push_str(&format!("extend schema @slo(class: \"{}\")\n\n", slo_class));

            // Add @key types and extend them with fields from subgraphs
            for t in 0..type_count {
                let type_name = format!("Type{}", t);

                if s == 0 {
                    // First subgraph: define the type with @key
                    subgraph_content
                        .push_str(&format!("type {} @key(fields: \"id\") {{\n", type_name));
                    subgraph_content.push_str("  id: ID!\n");

                    // Add some fields with SLO
                    let fields_in_subgraph = (fields_per_type as f64 * 0.3) as usize;
                    for f in 0..fields_in_subgraph {
                        let field_name = format!("field_{}", f);
                        if (next_rand(&mut rng_state) as f64 / u64::MAX as f64) < slo_field_ratio {
                            let field_slo = SLO_CLASSES
                                [(next_rand(&mut rng_state) as usize) % SLO_CLASSES.len()];
                            subgraph_content.push_str(&format!(
                                "  {}: String @slo(class: \"{}\")\n",
                                field_name, field_slo
                            ));
                        } else {
                            subgraph_content.push_str(&format!("  {}: String\n", field_name));
                        }
                    }
                    subgraph_content.push_str("}\n");
                } else {
                    // Subsequent subgraphs: extend the type
                    subgraph_content.push_str(&format!(
                        "extend type {} @key(fields: \"id\") {{\n",
                        type_name
                    ));
                    subgraph_content.push_str("  id: ID! @external\n");

                    // Add different fields with SLO
                    let fields_in_subgraph = (fields_per_type as f64 * 0.3) as usize;
                    for f in 0..fields_in_subgraph {
                        let field_name = format!("sg{}_field_{}", s, f);
                        if (next_rand(&mut rng_state) as f64 / u64::MAX as f64) < slo_field_ratio {
                            let field_slo = SLO_CLASSES
                                [(next_rand(&mut rng_state) as usize) % SLO_CLASSES.len()];
                            subgraph_content.push_str(&format!(
                                "  {}: String @slo(class: \"{}\")\n",
                                field_name, field_slo
                            ));
                        } else {
                            subgraph_content.push_str(&format!("  {}: String\n", field_name));
                        }
                    }
                    subgraph_content.push_str("}\n");
                }
            }

            // Add Query extension with entry points
            subgraph_content.push_str("\nextend type Query {\n");
            for t in 0..type_count.min(5) {
                subgraph_content.push_str(&format!("  getType{}: Type{}\n", t, t));
            }
            subgraph_content.push_str("}\n");

            fs::write(&subgraph_path, subgraph_content).unwrap();
        }

        // Create main federated schema
        let schema_path = project_dir.join("schema.graphql");
        let mut schema_content = String::new();

        // Add federation directives
        schema_content.push_str("directive @key(fields: String!) on OBJECT | INTERFACE\n");
        schema_content.push_str("directive @external on FIELD_DEFINITION\n");
        schema_content.push_str("directive @slo(class: String!) on FIELD_DEFINITION | SCHEMA\n\n");

        // Define types with @key (these are the base types that subgraphs extend)
        for t in 0..type_count {
            schema_content.push_str(&format!("type Type{} @key(fields: \"id\") {{\n", t));
            schema_content.push_str("  id: ID!\n");
            // Main schema has reference to fields, but without SLO (SLO is in subgraphs)
            for f in 0..fields_per_type {
                schema_content.push_str(&format!("  field_{}: String\n", f));
            }
            schema_content.push_str("}\n");
        }

        // Add Query type
        schema_content.push_str("type Query {\n");
        for t in 0..type_count {
            schema_content.push_str(&format!("  getType{}: Type{}\n", t, t));
        }
        schema_content.push_str("}\n");
        schema_content.push_str("schema { query: Query }\n");

        fs::write(&schema_path, schema_content).unwrap();

        // Create operation files
        for j in 0..files_per_project {
            let file_path = project_dir.join(format!("file_{}.graphql", j));

            let query = format!("query Q{} {{\n  getType0 {{ id field_0 }}\n}}", j);

            let fragments: String = (0..2)
                .map(|f| {
                    format!(
                        "\nfragment Frag{}_{}_{} on Type{} {{ id field_{} }}",
                        i,
                        j,
                        f,
                        f % type_count,
                        f % fields_per_type
                    )
                })
                .collect::<String>();

            let content = format!("{}{}", query, fragments);
            fs::write(file_path, content).unwrap();
        }

        projects.push(
            ProjectConfig::default()
                .with_schema(SchemaSource::Single(format!(
                    "project_{}/schema.graphql",
                    i
                )))
                .with_include(GlobPattern::Single(format!("project_{}/**/*.graphql", i)))
                .with_subgraphs_dir(format!("project_{}/subgraphs", i)),
        );
    }

    Config::new_empty()
        .with_projects(projects)
        .with_base_dir(base_dir.to_path_buf())
        .with_enable_schema_cache(true)
}

// ============================================================================
// Workspace Scan Benchmark
// ============================================================================

fn bench_subgraph_workspace_scan(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let mut group = c.benchmark_group("Subgraph SLO / Workspace Scan");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_millis(100));

    // Match baseline: 10 projects, 50 files, 50 types × 20 fields, plus 10 subgraphs
    group.bench_function("500 files, 10 projects, 10 subgraphs, 50% SLO", |b| {
        b.iter(|| {
            let config = generate_subgraph_workspace(
                &base_dir, 10,  // projects
                50,  // files per project
                50,  // types
                20,  // fields per type
                10,  // subgraphs per project
                0.5, // 50% SLO fields
                42,  // stable seed
            );
            rt.block_on(async {
                graphox::engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
            })
        })
    });

    group.finish();
}

// ============================================================================
// LSP Actions with Subgraphs Benchmark
// ============================================================================

fn bench_subgraph_lsp_actions(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    // Generate workspace once
    let config = generate_subgraph_workspace(
        &base_dir, 10,  // projects
        50,  // files per project
        50,  // types
        20,  // fields per type
        10,  // subgraphs per project
        0.5, // 50% SLO fields
        42,  // stable seed
    );

    // Enter runtime before creating Backend
    let _guard = rt.enter();
    let (service, _) = LspService::new(|client| Backend::new(client, config.clone()));
    let backend = service.inner();

    // Pre-populate all documents
    rt.block_on(async {
        for i in 0..10 {
            for j in 0..50 {
                let path = base_dir.join(format!("project_{}/file_{}.graphql", i, j));
                let uri = Url::from_file_path(fs::canonicalize(&path).unwrap()).unwrap();
                let content = fs::read_to_string(&path).unwrap();
                let doc = DocumentState::new_from_thread_local(
                    uri.clone(),
                    &content,
                    PositionEncodingKind::UTF8,
                );
                backend.documents.insert(uri, std::sync::Arc::new(doc));
            }
        }
    });

    // Target: query field in project_0/file_0.graphql
    let query_uri = Url::from_file_path(base_dir.join("project_0/file_0.graphql")).unwrap();

    // Target: field in main schema (project_0/schema.graphql)
    let schema_uri = Url::from_file_path(base_dir.join("project_0/schema.graphql")).unwrap();

    let mut group = c.benchmark_group("Subgraph SLO / LSP Actions");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_millis(100));

    // Completion benchmark - at query field position
    group.bench_function("Completion (SLO-aware)", |b| {
        b.to_async(&rt).iter(|| async {
            backend
                .completion(CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier {
                            uri: query_uri.clone(),
                        },
                        position: Position::new(0, 10), // at field_0
                    },
                    context: None,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
        })
    });

    // Hover benchmark - at query field position
    group.bench_function("Hover (with SLO info)", |b| {
        b.to_async(&rt).iter(|| async {
            backend
                .hover(HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier {
                            uri: query_uri.clone(),
                        },
                        position: Position::new(0, 10), // at field_0
                    },
                    work_done_progress_params: Default::default(),
                })
                .await
        })
    });

    // Goto Definition: Query field -> Main schema
    group.bench_function("Goto Def: Query -> Main Schema", |b| {
        b.to_async(&rt).iter(|| async {
            backend
                .goto_definition(GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier {
                            uri: query_uri.clone(),
                        },
                        position: Position::new(0, 10), // at field_0 in query
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
        })
    });

    // Goto Definition: Main schema field -> Subgraph
    // This tests navigation from main schema field definition to subgraph schema
    group.bench_function("Goto Def: Main Schema -> Subgraph", |b| {
        b.to_async(&rt).iter(|| async {
            backend
                .goto_definition(GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier {
                            uri: schema_uri.clone(),
                        },
                        // Position in schema.graphql at a field with SLO
                        // Need to find a position that lands on field_0 in Type0
                        position: Position::new(15, 8), // Adjust based on actual schema structure
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
        })
    });

    group.finish();
}

// ============================================================================
// Incremental Scan Benchmark
// ============================================================================

fn bench_subgraph_incremental_scan(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let config = generate_subgraph_workspace(
        &base_dir, 10,  // projects
        50,  // files per project
        50,  // types
        20,  // fields per type
        10,  // subgraphs per project
        0.5, // 50% SLO fields
        42,  // stable seed
    );

    // Initial scan
    let initial_metadata = rt.block_on(async {
        graphox::engine::Engine::scan_workspace(&config, PositionEncodingKind::UTF8, None)
    });

    let mut group = c.benchmark_group("Subgraph SLO / Incremental Scan");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_millis(100));

    group.bench_function("Rescan (no changes)", |b| {
        b.iter(|| {
            rt.block_on(async {
                graphox::engine::Engine::scan_workspace(
                    &config,
                    PositionEncodingKind::UTF8,
                    Some(&initial_metadata),
                )
            })
        })
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_millis(800))
        .measurement_time(std::time::Duration::from_millis(1000));
    targets =
        bench_subgraph_workspace_scan,
        bench_subgraph_lsp_actions,
        bench_subgraph_incremental_scan,
);
criterion_main!(benches);
