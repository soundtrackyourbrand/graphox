use colored::*;
use fnv::FnvHashMap as HashMap;
use graphql_rust::Config;
use graphql_rust::engine::Engine;
use rayon::prelude::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub async fn run_benchmark(config: Config, _verbose: bool) {
    println!("{}", "Starting Benchmark...".bold());
    let total_start = Instant::now();

    // 1. Discovery & Metadata Collection (Parallel)
    let workspace_metadata = Engine::scan_workspace(&config, |_, _| {});
    let global_metadata = &workspace_metadata.fragments;
    let scan_timings = &workspace_metadata.timings;

    let mut fragment_to_path_global: HashMap<String, String> = HashMap::default();
    for meta in global_metadata {
        fragment_to_path_global.insert(meta.name.clone(), meta.path.clone());
    }

    let all_graphql_paths: Vec<_> = global_metadata
        .iter()
        .map(|m| PathBuf::from(&m.path))
        .collect();
    let _ = all_graphql_paths;

    // 2. Project Processing
    let mut total_graphql_files = 0;
    let mut total_operations = 0;
    let mut total_fragments_processed = 0;
    let mut schema_parse_time = Duration::ZERO;
    let mut fragment_resolve_time = Duration::ZERO;
    let mut doc_parse_time = Duration::ZERO;
    let mut ts_gen_time = Duration::ZERO;
    let mut metadata_mapping_time = Duration::ZERO;

    let mut project_timings = Vec::new();
    let mut schema_type_timings = Vec::new();

    for (project, project_meta) in config.projects.iter().zip(&workspace_metadata.projects) {
        let project_total_start = Instant::now();
        let sp_start = Instant::now();

        let valid_schema = match graphql_rust::schema::load_and_validate_schema(&config.base_dir, &project.schema) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}", e.to_string().red());
                continue;
            }
        };
        schema_parse_time += sp_start.elapsed();

        let fr_start = Instant::now();
        let project_context =
            Engine::resolve_project_context(&valid_schema, global_metadata, &project_meta.files);
        fragment_resolve_time += fr_start.elapsed();

        let project_files = &project_meta.files;

        // Project-specific maps
        let mm_start = Instant::now();
        // The mapping is now part of project_context
        let project_fragment_to_path = &project_context.fragment_to_path;
        let project_fragment_to_import = &project_context.fragment_to_import;
        let all_fragments = &project_context.all_fragments;
        metadata_mapping_time += mm_start.elapsed();

        let (p_graphql_files, p_operations, p_fragments_processed, p_doc_parse_time, p_ts_gen_time) =
            project_files
                .par_iter()
                .map(|path| {
                    let dp_start = Instant::now();
                    let doc_opt = workspace_metadata.documents.get(path);
                    let d_time = dp_start.elapsed();

                    if let Some(doc) = doc_opt {
                        let schema_import = config.schema_types.as_ref().and_then(|sts| {
                            sts.iter()
                                .find(|st| st.schema.as_key() == project.schema.as_key())
                                .and_then(|st| st.import.clone())
                        });
                        let ctx = graphql_rust::features::codegen::CodegenContext {
                            schema: &valid_schema,
                            fragment_to_path: project_fragment_to_path,
                            fragment_to_import: project_fragment_to_import,
                            fragment_to_type_only: &project_context.fragment_to_type_only,
                            all_fragments,
                            current_file_path: path,
                            scalars: &config.scalars,
                            schema_import: &schema_import,
                            generate_ast_for_fragments: config
                                .generate_ast_for_fragments
                                .unwrap_or(false),
                        };
                        let g_start = Instant::now();
                        if let Ok(_ts_code) =
                            graphql_rust::features::codegen::generate_typescript(doc, &ctx)
                        {
                            let g_time = g_start.elapsed();
                            return (
                                1,
                                doc.get_graphql_trees().len(),
                                doc.fragments().len(),
                                d_time,
                                g_time,
                            );
                        }
                        (1, 0, 0, d_time, Duration::ZERO)
                    } else {
                        (0, 0, 0, d_time, Duration::ZERO)
                    }
                })
                .reduce(
                    || (0, 0, 0, Duration::ZERO, Duration::ZERO),
                    |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3, a.4 + b.4),
                );

        total_graphql_files += p_graphql_files;
        total_operations += p_operations;
        total_fragments_processed += p_fragments_processed;
        doc_parse_time += p_doc_parse_time;
        ts_gen_time += p_ts_gen_time;
        project_timings.push((project.include.as_key(), project_total_start.elapsed()));
    }

    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            let st_start = Instant::now();
            if let Ok(valid_schema) = graphql_rust::schema::load_and_validate_schema(&config.base_dir, &st.schema) {
                let g_start = Instant::now();
                let _ts_code = graphql_rust::features::codegen::generate_schema_types(
                    &valid_schema,
                    &config.scalars,
                );
                ts_gen_time += g_start.elapsed();
            }
            schema_type_timings.push((st.output.clone(), st_start.elapsed()));
        }
    }

    let total_duration = total_start.elapsed();

    println!("\n{}", "--- Benchmark Results ---".bold());
    println!(
        "{:<30} {}",
        "Files with GraphQL:".bright_black(),
        total_graphql_files
    );
    println!(
        "{:<30} {}",
        "Total Fragments Found:".bright_black(),
        fragment_to_path_global.len()
    );
    println!(
        "{:<30} {}",
        "Total Operations processed:".bright_black(),
        total_operations
    );
    println!(
        "{:<30} {}",
        "Total Fragments processed:".bright_black(),
        total_fragments_processed
    );
    println!();
    if !project_timings.is_empty() {
        println!("{}", "Project Breakdown:".bold());
        for (key, duration) in project_timings {
            println!("  {:30}: {:>10?}", key.blue(), duration);
        }
        println!();
    }
    if !schema_type_timings.is_empty() {
        println!("{}", "Schema Types Breakdown:".bold());
        for (key, duration) in schema_type_timings {
            println!("  {:30}: {:>10?}", key.blue(), duration);
        }
        println!();
    }
    println!("{}", "Phase Timings:".bold());
    println!(
        "  {:<26} {:>10?}",
        "Workspace Glob Resolution:".bright_black(),
        scan_timings.glob_resolution
    );
    println!(
        "  {:<26} {:>10?}",
        "Workspace Doc Parsing:".bright_black(),
        scan_timings.doc_parsing
    );
    println!(
        "  {:<26} {:>10?}",
        "Workspace Metadata Extr:".bright_black(),
        scan_timings.metadata_extraction
    );
    println!(
        "  {:<26} {:>10?}",
        "Schema Parsing:".bright_black(),
        schema_parse_time
    );
    println!(
        "  {:<26} {:>10?}",
        "Fragment Resolution:".bright_black(),
        fragment_resolve_time
    );
    println!(
        "  {:<26} {:>10?}",
        "Metadata Mapping:".bright_black(),
        metadata_mapping_time
    );
    println!(
        "  {:<26} {:>10?}",
        "Document Parsing:".bright_black(),
        doc_parse_time
    );
    println!(
        "  {:<26} {:>10?}",
        "TS Generation:".bright_black(),
        ts_gen_time
    );
    println!("{}", "--------------------------".bright_black());
    println!("{:<30} {:>10?}", "Total Wall Time:".bold(), total_duration);
}
