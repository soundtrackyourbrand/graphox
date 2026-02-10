use ahash::AHashMap as HashMap;
use colored::*;
use graphox_codegen as codegen;
use graphox_core::Config;
use graphox_core::engine::Engine;
use graphox_core::schema;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub async fn run_benchmark(config: Config, _verbose: bool) {
    println!("{}", "Starting Benchmark...".bold());
    let total_start = Instant::now();

    let workspace_metadata = Engine::scan_workspace(
        &config,
        tower_lsp::lsp_types::PositionEncodingKind::UTF8,
        None,
    );
    let global_metadata = &workspace_metadata.fragments;
    let scan_timings = &workspace_metadata.timings;

    let mut fragment_to_path_global: HashMap<Arc<str>, Arc<str>> = HashMap::default();
    for meta in global_metadata {
        fragment_to_path_global.insert(meta.name.clone(), meta.path.clone());
    }

    let all_graphql_paths: Vec<_> = global_metadata
        .iter()
        .map(|m| PathBuf::from(m.path.as_ref()))
        .collect();
    let _ = all_graphql_paths;

    let mut total_graphql_files = 0;
    let mut total_operations = 0;
    let mut total_fragments_processed = 0;
    let mut schema_parse_time = Duration::ZERO;
    let mut fragment_resolve_time = Duration::ZERO;
    let mut doc_parse_time = Duration::ZERO;
    let mut ts_gen_time = Duration::ZERO;
    let mut metadata_mapping_time = Duration::ZERO;
    let mut codegen_profile = codegen::CodegenProfile::default();

    let mut project_timings = Vec::new();
    let mut schema_type_timings = Vec::new();

    for (project, project_meta) in config.projects.iter().zip(&workspace_metadata.projects) {
        let project_total_start = Instant::now();
        let sp_start = Instant::now();

        let valid_schema = match schema::load_and_validate_schema(&config.base_dir, &project.schema)
        {
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

        let mm_start = Instant::now();
        let project_fragment_to_path = &project_context.fragment_to_path;
        let project_fragment_to_import = &project_context.fragment_to_import;
        let all_fragments = &project_context.all_fragments;
        metadata_mapping_time += mm_start.elapsed();

        let shared_type_cache = codegen::TypeCache::new();

        let (
            p_graphql_files,
            p_operations,
            p_fragments_processed,
            p_doc_parse_time,
            p_ts_gen_time,
            p_profile,
        ) = project_files
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
                    let ctx = codegen::CodegenContext::new(
                        &valid_schema,
                        project_fragment_to_path,
                        project_fragment_to_import,
                        &project_context.fragment_to_type_only,
                        all_fragments,
                        path,
                        &config.scalars,
                        &schema_import,
                        config.generate_ast_for_fragments.unwrap_or(false),
                        &project_context.fragment_dependencies,
                        &shared_type_cache,
                        "Document",
                        "Variables",
                        "",
                        codegen::FragmentMasking::Disabled,
                        "./fragment-masking".to_string(),
                    );
                    let g_start = Instant::now();
                    if let Ok((_ts_code, _ops, profile)) =
                        codegen::generate_typescript_with_profile(doc, &ctx)
                    {
                        let g_time = g_start.elapsed();
                        return (
                            1,
                            doc.get_graphql_trees().len(),
                            doc.fragments().len(),
                            d_time,
                            g_time,
                            profile,
                        );
                    }
                    (1, 0, 0, d_time, Duration::ZERO, Default::default())
                } else {
                    (0, 0, 0, d_time, Duration::ZERO, Default::default())
                }
            })
            .reduce(
                || {
                    (
                        0,
                        0,
                        0,
                        Duration::ZERO,
                        Duration::ZERO,
                        codegen::CodegenProfile::default(),
                    )
                },
                |a, b| {
                    (
                        a.0 + b.0,
                        a.1 + b.1,
                        a.2 + b.2,
                        a.3 + b.3,
                        a.4 + b.4,
                        codegen::CodegenProfile {
                            parse_time: a.5.parse_time + b.5.parse_time,
                            selection_set_time: a.5.selection_set_time + b.5.selection_set_time,
                            ast_serialization_time: a.5.ast_serialization_time
                                + b.5.ast_serialization_time,
                            import_generation_time: a.5.import_generation_time
                                + b.5.import_generation_time,
                        },
                    )
                },
            );

        total_graphql_files += p_graphql_files;
        total_operations += p_operations;
        total_fragments_processed += p_fragments_processed;
        doc_parse_time += p_doc_parse_time;
        ts_gen_time += p_ts_gen_time;
        codegen_profile.parse_time += p_profile.parse_time;
        codegen_profile.selection_set_time += p_profile.selection_set_time;
        codegen_profile.ast_serialization_time += p_profile.ast_serialization_time;
        codegen_profile.import_generation_time += p_profile.import_generation_time;

        let (cache_hits, cache_misses) = shared_type_cache.stats();
        let cache_size = shared_type_cache.len();

        project_timings.push((project.include.as_key(), project_total_start.elapsed()));

        if cache_hits + cache_misses > 0 {
            let hit_rate = if cache_hits + cache_misses > 0 {
                (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "  Type Cache: {} types, {} hits, {} misses ({:.1}% hit rate)",
                cache_size.to_string().blue(),
                cache_hits.to_string().green(),
                cache_misses.to_string().yellow(),
                hit_rate
            );
        }
    }

    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            let st_start = Instant::now();
            if let Ok(valid_schema) = schema::load_and_validate_schema(&config.base_dir, &st.schema)
            {
                let g_start = Instant::now();
                let _ts_code = codegen::generate_schema_types(&valid_schema, &config.scalars);
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
        "Fragment Deps Cache:".bright_black(),
        scan_timings.fragment_deps_computation
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

    if ts_gen_time > Duration::ZERO {
        println!();
        println!("{}", "Codegen Breakdown:".bold());
        println!(
            "  {:<26} {:>10?}",
            "  GraphQL Parsing:".bright_black(),
            codegen_profile.parse_time
        );
        println!(
            "  {:<26} {:>10?}",
            "  Selection Set Gen:".bright_black(),
            codegen_profile.selection_set_time
        );
        println!(
            "  {:<26} {:>10?}",
            "  AST Serialization:".bright_black(),
            codegen_profile.ast_serialization_time
        );
        println!(
            "  {:<26} {:>10?}",
            "  Import Generation:".bright_black(),
            codegen_profile.import_generation_time
        );
    }

    println!("{}", "--------------------------".bright_black());
    println!("{:<30} {:>10?}", "Total Wall Time:".bold(), total_duration);
}
