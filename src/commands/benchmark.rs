use apollo_compiler::{executable, Node};
use fnv::{FnvHashMap as HashMap, FnvHashSet as HashSet};
use graphql_rust::engine::Engine;
use graphql_rust::Config;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub async fn run_benchmark(
    config: Config,
    verbose: bool,
) {
    println!("Starting Benchmark...");
    let total_start = Instant::now();

    // 1. Discovery & Metadata Collection (Parallel)
    let workspace_metadata = Engine::scan_workspace(&config);
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
    let mut resolution_cache: HashMap<String, HashMap<String, Node<executable::Fragment>>> =
        HashMap::default();

    for (project, project_meta) in config.projects.iter().zip(&workspace_metadata.projects) {
        let project_total_start = Instant::now();
        let sp_start = Instant::now();

        let schema = match Engine::load_schema(&config.base_dir, &project.schema) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", e);
                continue;
            }
        };
        let valid_schema = match schema.clone().validate() {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Schema validation failed for {}: {}",
                    project.schema.as_key(),
                    e
                );
                continue;
            }
        };
        schema_parse_time += sp_start.elapsed();

        let fr_start = Instant::now();
        let all_fragments = if let Some(cached) =
            resolution_cache.get(&project.schema.as_key())
        {
            cached.clone()
        } else {
            let resolved = Engine::resolve_fragments(&valid_schema, global_metadata);
            resolution_cache.insert(project.schema.as_key(), resolved.clone());
            resolved
        };
        fragment_resolve_time += fr_start.elapsed();

        let project_files = &project_meta.files;
        let project_files_set: HashSet<String> = project_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // Project-specific maps
        let mm_start = Instant::now();
        let mut project_fragment_to_path: HashMap<String, String> = HashMap::default();
        let mut project_fragment_to_import: HashMap<String, String> = HashMap::default();

        for meta in global_metadata {
            let is_local = project_files_set.contains(&meta.path);
            if is_local {
                if verbose {
                    println!("Local Fragment Found: {} in {}", meta.name, meta.path);
                }
                project_fragment_to_path.insert(meta.name.clone(), meta.path.clone());
                if let Some(a) = &meta.import_alias {
                    project_fragment_to_import.insert(meta.name.clone(), a.clone());
                }
            } else if meta.is_public {
                if verbose {
                    println!(
                        "Public Global Fragment Found: {} from {}",
                        meta.name, meta.path
                    );
                }
                project_fragment_to_path
                    .entry(meta.name.clone())
                    .or_insert_with(|| meta.path.clone());
                if let Some(a) = &meta.import_alias {
                    project_fragment_to_import
                        .entry(meta.name.clone())
                        .or_insert_with(|| a.clone());
                }
            }
        }
        metadata_mapping_time += mm_start.elapsed();

        for path in project_files {
            let dp_start = Instant::now();
            let doc_opt = Engine::parse_doc(path);
            doc_parse_time += dp_start.elapsed();

            if let Some(doc) = doc_opt {
                total_graphql_files += 1;
                let schema_import = config.schema_types.as_ref().and_then(|sts| {
                    sts.iter()
                        .find(|st| st.schema.as_key() == project.schema.as_key())
                        .and_then(|st| st.import.clone())
                });
                let ctx = graphql_rust::features::codegen::CodegenContext {
                    schema: &schema,
                    fragment_to_path: &project_fragment_to_path,
                    fragment_to_import: &project_fragment_to_import,
                    all_fragments: &all_fragments,
                    current_file_path: path,
                    scalars: &config.scalars,
                    schema_import: &schema_import,
                    generate_ast_for_fragments: config.generate_ast_for_fragments.unwrap_or(false),
                };
                let g_start = Instant::now();
                if let Ok(_ts_code) =
                    graphql_rust::features::codegen::generate_typescript(&doc, &ctx)
                {
                    ts_gen_time += g_start.elapsed();
                    total_operations += doc.get_graphql_trees().len();
                    total_fragments_processed += doc.fragments().len();
                }
            }
        }
        project_timings.push((project.include.as_key(), project_total_start.elapsed()));
    }

    if let Some(schema_types) = &config.schema_types {
        for st in schema_types {
            let st_start = Instant::now();
            if let Ok(schema) = Engine::load_schema(&config.base_dir, &st.schema) {
                let g_start = Instant::now();
                let _ts_code =
                    graphql_rust::features::codegen::generate_schema_types(&schema, &config.scalars);
                ts_gen_time += g_start.elapsed();
            }
            schema_type_timings.push((st.output.clone(), st_start.elapsed()));
        }
    }

    let total_duration = total_start.elapsed();

    println!("\n--- Benchmark Results ---");
    println!("Files with GraphQL:       {}", total_graphql_files);
    println!("Total Fragments Found:    {}", fragment_to_path_global.len());
    println!("Total Operations processed: {}", total_operations);
    println!("Total Fragments processed:  {}", total_fragments_processed);
    println!();
    if !project_timings.is_empty() {
        println!("Project Breakdown:");
        for (key, duration) in project_timings {
            println!("  {:30}: {:>10?}", key, duration);
        }
        println!();
    }
    if !schema_type_timings.is_empty() {
        println!("Schema Types Breakdown:");
        for (key, duration) in schema_type_timings {
            println!("  {:30}: {:>10?}", key, duration);
        }
        println!();
    }
    println!("Phase Timings:");
    println!(
        "  Workspace Glob Resolution: {:>10?}",
        scan_timings.glob_resolution
    );
    println!(
        "  Workspace Doc Parsing:     {:>10?}",
        scan_timings.doc_parsing
    );
    println!(
        "  Workspace Metadata Extr:   {:>10?}",
        scan_timings.metadata_extraction
    );
    println!("  Schema Parsing:            {:>10?}", schema_parse_time);
    println!("  Fragment Resolution:       {:>10?}", fragment_resolve_time);
    println!("  Metadata Mapping:          {:>10?}", metadata_mapping_time);
    println!("  Document Parsing:          {:>10?}", doc_parse_time);
    println!("  TS Generation (serial):    {:>10?}", ts_gen_time);
    println!("--------------------------");
    println!("Total Wall Time:             {:>10?}", total_duration);
}
