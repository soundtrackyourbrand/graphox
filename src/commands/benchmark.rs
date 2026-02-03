use fnv::{FnvHashMap as HashMap, FnvHashSet as HashSet};
use graphql_rust::engine::Engine;
use graphql_rust::Config;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub async fn run_benchmark(
    mut config: Option<Config>,
    _schema_path: &str,
    scan_path: &str,
    verbose: bool,
) {
    if config.is_none() {
        config = Config::load_from_dir(scan_path);
    }

    println!("Starting Benchmark...");
    let total_start = Instant::now();

    // 1. Discovery & Metadata Collection (Parallel)
    let discovery_start = Instant::now();
    let workspace_metadata = if let Some(cfg) = &config {
        Engine::scan_workspace(cfg)
    } else {
        graphql_rust::engine::WorkspaceMetadata {
            fragments: vec![],
            operations: vec![],
        }
    };
    let global_metadata = &workspace_metadata.fragments;
    let file_discovery_time = discovery_start.elapsed();

    let mut fragment_to_path_global: HashMap<String, String> = HashMap::default();
    for meta in global_metadata {
        fragment_to_path_global.insert(meta.name.clone(), meta.path.clone());
    }

    let all_graphql_paths: Vec<_> = global_metadata
        .iter()
        .map(|m| PathBuf::from(&m.path))
        .collect();

    // 2. Project Processing
    let mut total_graphql_files = 0;
    let mut total_operations = 0;
    let mut total_fragments_processed = 0;
    let mut schema_parse_time = Duration::ZERO;
    let mut fragment_resolve_time = Duration::ZERO;
    let mut ts_gen_time = Duration::ZERO;

    if let Some(cfg) = &config {
        for project in &cfg.projects {
            let sp_start = Instant::now();
            let schema = match Engine::load_schema(&cfg.base_dir, &project.schema) {
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
            let all_fragments = Engine::resolve_fragments(&valid_schema, &all_graphql_paths);
            fragment_resolve_time += fr_start.elapsed();

            let abs_includes: Vec<String> = project
                .include
                .patterns()
                .iter()
                .map(|p| cfg.base_dir.join(p).to_string_lossy().to_string())
                .collect();
            let abs_excludes: Vec<String> = project
                .exclude
                .as_ref()
                .map(|e| e.patterns())
                .unwrap_or_default()
                .iter()
                .map(|p| cfg.base_dir.join(p).to_string_lossy().to_string())
                .collect();
            let project_files = graphql_rust::utils::get_project_files(&abs_includes, &abs_excludes);
            let project_files_set: HashSet<String> = project_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            // Project-specific maps
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

            for path in project_files {
                if let Some(doc) = Engine::parse_doc(&path) {
                    total_graphql_files += 1;
                    let schema_import = cfg.schema_types.as_ref().and_then(|sts| {
                        sts.iter()
                            .find(|st| st.schema.as_key() == project.schema.as_key())
                            .and_then(|st| st.import.clone())
                    });
                    let ctx = graphql_rust::features::codegen::CodegenContext {
                        schema: &schema,
                        fragment_to_path: &project_fragment_to_path,
                        fragment_to_import: &project_fragment_to_import,
                        all_fragments: &all_fragments,
                        current_file_path: &path,
                        scalars: &cfg.scalars,
                        schema_import: &schema_import,
                        generate_ast_for_fragments: cfg.generate_ast_for_fragments.unwrap_or(false),
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
        }
    }
    let total_duration = total_start.elapsed();

    println!("\n--- Benchmark Results ---");
    println!("Files with GraphQL:       {}", total_graphql_files);
    println!("Total Fragments Found:    {}", fragment_to_path_global.len());
    println!("Total Operations processed: {}", total_operations);
    println!("Total Fragments processed:  {}", total_fragments_processed);
    println!();
    println!("Phase Timings:");
    println!("  File Discovery & Metadata: {:>10?}", file_discovery_time);
    println!("  Schema Parsing:            {:>10?}", schema_parse_time);
    println!("  Fragment Resolution:       {:>10?}", fragment_resolve_time);
    println!("  TS Generation (serial):    {:>10?}", ts_gen_time);
    println!("--------------------------");
    println!("Total Wall Time:             {:>10?}", total_duration);
}
