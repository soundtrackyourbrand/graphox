use graphql_rust::utils::is_relevant_file;
use graphql_rust::{Config, DocumentLanguage, DocumentState};
use graphql_rust::engine::Engine;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tower_lsp::lsp_types::Url;
use rayon::prelude::*;
use std::path::PathBuf;

pub async fn run_benchmark(mut config: Option<Config>, _schema_path: &str, scan_path: &str, verbose: bool) {
    if config.is_none() {
        config = Config::load_from_dir(scan_path);
    }
    
    println!("Starting Benchmark...");
    let total_start = Instant::now();

    // 1. Discovery & Initial Scan
    let discovery_start = Instant::now();
    let scan_root = if let Some(cfg) = &config {
        let mut all_paths = Vec::new();
        for project in &cfg.projects {
            let abs_include = cfg.base_dir.join(&project.include).to_string_lossy().to_string();
            let paths = graphql_rust::utils::get_project_files(&abs_include);
            for path in paths {
                if is_relevant_file(&path) {
                    all_paths.push((path, project.import.clone()));
                }
            }
        }
        all_paths
    } else {
        graphql_rust::utils::get_project_files(scan_path)
            .into_iter()
            .map(|p| (p, None))
            .collect()
    };
    let file_discovery_time = discovery_start.elapsed();

    let total_files_scanned = scan_root.len();
    let mut total_graphql_files = 0;
    let mut fragment_to_path: HashMap<String, String> = HashMap::new();
    let mut fragment_to_import: HashMap<String, String> = HashMap::new();

    let scan_start = Instant::now();
    let scan_results: Vec<_> = scan_root
        .par_iter()
        .map(|(path, import_alias)| {
            let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            if let Some(doc) = Engine::parse_doc(&abs_path) {
                let mut fragments = Vec::new();
                for frag in doc.fragments() {
                    fragments.push((
                        frag.name.clone(),
                        abs_path.to_string_lossy().to_string(),
                        import_alias.clone(),
                    ));
                }
                Some((true, fragments, false, path.to_string_lossy().to_string()))
            } else {
                let skipped = DocumentLanguage::from_uri(&Url::from_file_path(path).unwrap()).is_host_language();
                Some((false, vec![], skipped, path.to_string_lossy().to_string()))
            }
        })
        .collect();

    for res in scan_results.iter().flatten() {
        let (has_gql, frags, skipped, path) = res;
        if verbose {
            if *skipped { println!("File skipped by fast check: {}", path); }
            else if !*has_gql { println!("File parsed but no GraphQL blocks found: {}", path); }
        }
        if *has_gql {
            total_graphql_files += 1;
        }
        for (name, path, alias) in frags {
            if verbose { println!("Fragment Found: {} in {}", name, path); }
            fragment_to_path.insert(name.clone(), path.clone());
            if let Some(a) = alias { fragment_to_import.insert(name.clone(), a.clone()); }
        }
    }
    let parallel_scan_time = scan_start.elapsed();

    // 2. Project Processing
    let mut total_operations = 0;
    let mut total_fragments_processed = 0;
    let mut schema_parse_time = Duration::ZERO;
    let mut fragment_resolve_time = Duration::ZERO;
    let mut ts_gen_time = Duration::ZERO;

    let all_graphql_paths: Vec<_> = fragment_to_path.values().map(PathBuf::from).collect();

    if let Some(cfg) = &config {
        for project in &cfg.projects {
            let s_start = Instant::now();
            let schema = match Engine::load_schema(&cfg.base_dir, &project.schema) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let valid_schema = match schema.clone().validate() {
                Ok(v) => v,
                Err(_) => continue,
            };
            schema_parse_time += s_start.elapsed();

            let fr_start = Instant::now();
            let all_fragments = Engine::resolve_fragments(&valid_schema, &all_graphql_paths);
            fragment_resolve_time += fr_start.elapsed();

            let abs_include = cfg.base_dir.join(&project.include).to_string_lossy().to_string();
            let paths = graphql_rust::utils::get_project_files(&abs_include);
            for path in paths {
                if let Some(doc) = Engine::parse_doc(&path) {
                    let schema_import = cfg.schema_types.as_ref().and_then(|sts| {
                        sts.iter().find(|st| st.schema.as_key() == project.schema.as_key()).and_then(|st| st.import.clone())
                    });
                    let ctx = graphql_rust::features::codegen::CodegenContext {
                        schema: &schema,
                        fragment_to_path: &fragment_to_path,
                        fragment_to_import: &fragment_to_import,
                        all_fragments: &all_fragments,
                        current_file_path: &path,
                        scalars: &cfg.scalars,
                        schema_import: &schema_import,
                    };
                    let g_start = Instant::now();
                    if let Ok(_ts_code) = graphql_rust::features::codegen::generate_typescript(&doc, &ctx) {
                        ts_gen_time += g_start.elapsed();
                        total_operations += doc.get_graphql_trees().len(); 
                        total_fragments_processed += doc.fragments().len();
                    }
                }
            }
        }
    }
    let total_duration = total_start.elapsed();
    let processing_duration = total_duration - file_discovery_time - parallel_scan_time;

    println!("\n--- Benchmark Results ---");
    println!("Total Files Scanned:      {}", total_files_scanned);
    println!("Files with GraphQL:       {}", total_graphql_files);
    println!("Total Fragments Found:    {}", fragment_to_path.len());
    println!("Total Operations processed: {}", total_operations);
    println!("Total Fragments processed:  {}", total_fragments_processed);
    println!("");
    println!("Phase Timings:");
    println!("  File Discovery:         {:>10?}", file_discovery_time);
    println!("  Parallel Scan & Parse:  {:>10?}", parallel_scan_time);
    println!("  Schema Parsing:         {:>10?}", schema_parse_time);
    println!("  Fragment Resolution:    {:>10?}", fragment_resolve_time);
    println!("  TS Generation (serial): {:>10?}", ts_gen_time);
    println!("  Total Processing:       {:>10?}", processing_duration);
    println!("--------------------------");
    println!("Total Wall Time:          {:>10?}", total_duration);
}
