use apollo_compiler::Schema;
use graphql_rust::utils::is_relevant_file;
use graphql_rust::{Config, DocumentLanguage, DocumentState};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tower_lsp::lsp_types::Url;
use rayon::prelude::*;

pub async fn run_benchmark(mut config: Option<Config>, _schema_path: &str, scan_path: &str, verbose: bool) {
    if config.is_none() {
        config = Config::load_from_dir(scan_path);
    }
    
    println!("Starting Benchmark...");
    let total_start = Instant::now();

    // 1. Initial Scan & Fragment Metadata Collection (Parallel)
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
    let mut fragment_to_path = HashMap::new();
    let mut fragment_to_import = HashMap::new();

    let scan_start = Instant::now();
    let scan_results: Vec<_> = scan_root
        .par_iter()
        .map(|(path, import_alias)| {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            let uri = Url::from_file_path(&abs_path).unwrap();
            let language = DocumentLanguage::from_uri(&uri);

            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&language.get_parser_language())
                .unwrap();
            
            let doc = DocumentState::new(uri, &content, parser);
            let mut fragments = Vec::new();
            let trees = doc.get_graphql_trees();
            
            let skipped_by_fast_check = doc.language.is_host_language() && !doc.has_graphql_candidates();

            if !trees.is_empty() {
                for frag in doc.fragments() {
                    fragments.push((
                        frag.name.clone(),
                        abs_path.to_string_lossy().to_string(),
                        import_alias.clone(),
                    ));
                }
                Some((true, fragments, skipped_by_fast_check, path.to_string_lossy().to_string()))
            } else {
                Some((false, vec![], skipped_by_fast_check, path.to_string_lossy().to_string()))
            }
        })
        .collect();

    for res in scan_results.iter().flatten() {
        let (has_gql, frags, skipped, path) = res;
        
        if verbose {
            if *skipped {
                println!("File skipped by fast check: {}", path);
            } else if !*has_gql {
                println!("File parsed but no GraphQL blocks found: {}", path);
            } else if frags.is_empty() {
                println!("File matched but yielded no fragments (only operations?): {}", path);
            }
        }

        if *has_gql {
            total_graphql_files += 1;
        }
        
        for (name, path, alias) in frags {
            if verbose {
                println!("Fragment Found: {} in {}", name, path);
            }
            fragment_to_path.insert(name.clone(), path.clone());
            if let Some(a) = alias {
                fragment_to_import.insert(name.clone(), a.clone());
            }
        }
    }
    let parallel_scan_time = scan_start.elapsed();

    // 2. Project Processing
    let processing_start = Instant::now();
    let mut total_operations = 0;
    let mut total_fragments_processed = 0;
    let mut schema_parse_time = Duration::ZERO;
    let mut fragment_resolve_time = Duration::ZERO;
    let mut ts_gen_time = Duration::ZERO;

    if let Some(cfg) = &config {
        for project in &cfg.projects {
            let abs_include = cfg.base_dir.join(&project.include).to_string_lossy().to_string();
            
            let s_start = Instant::now();
            let mut combined_text = String::new();
            for file in project.schema.files() {
                if let Ok(t) = std::fs::read_to_string(cfg.base_dir.join(file)) {
                    combined_text.push_str(&t);
                    combined_text.push('\n');
                }
            }
            let schema = match Schema::parse(&combined_text, &project.schema.as_key()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to parse schema {}: {}", project.schema.as_key(), e);
                    continue;
                }
            };
            let valid_schema = match schema.clone().validate() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Schema validation failed for {}: {}", project.schema.as_key(), e);
                    continue;
                }
            };
            schema_parse_time += s_start.elapsed();

            // Parallel Transitive Fragment Resolving
            let fr_start = Instant::now();
            let mut all_fragments = HashMap::new();
            let all_graphql_files: Vec<_> = scan_results
                .iter()
                .flatten()
                .filter(|(has_gql, _, _, _)| *has_gql)
                .map(|(_, _, _, path)| path.clone())
                .collect();

            let fragment_results: Vec<_> = all_graphql_files
                .par_iter()
                .map(|path_str| {
                    let content = std::fs::read_to_string(path_str).unwrap_or_default();
                    let abs_path = std::fs::canonicalize(path_str).unwrap_or_else(|_| std::path::PathBuf::from(path_str));
                    let uri = Url::from_file_path(&abs_path).unwrap();
                    let language = DocumentLanguage::from_uri(&uri);
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&language.get_parser_language()).unwrap();
                    let doc = DocumentState::new(uri, &content, parser);
                    
                    let mut frags = Vec::new();
                    for block in doc.get_graphql_trees() {
                        let block_text = doc.get_node_text(block.tree.root_node(), block.offset);
                        let masked = graphql_rust::utils::mask_interpolations(&block_text);
                        if let Ok(exec_doc) = apollo_compiler::executable::ExecutableDocument::parse(&valid_schema, &masked, "doc.graphql") {
                            for (name, frag) in exec_doc.fragments {
                                frags.push((name.to_string(), frag.clone()));
                            }
                        }
                    }
                    frags
                })
                .collect();

            for frags in fragment_results {
                for (name, frag) in frags {
                    all_fragments.insert(name, frag);
                }
            }
            fragment_resolve_time += fr_start.elapsed();

            let paths = graphql_rust::utils::get_project_files(&abs_include);
            for path in paths {
                if is_relevant_file(&path) {
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    let abs_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    let uri = Url::from_file_path(&abs_path).unwrap();
                    let language = DocumentLanguage::from_uri(&uri);
                    
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&language.get_parser_language()).unwrap();
                    let doc = DocumentState::new(uri, &content, parser);
                    
                    if !doc.get_graphql_trees().is_empty() {
                        let schema_import = cfg.schema_types.as_ref().and_then(|sts| {
                            sts.iter()
                                .find(|st| st.schema.as_key() == project.schema.as_key())
                                .and_then(|st| st.import.clone())
                        });

                        let ctx = graphql_rust::features::codegen::CodegenContext {
                            schema: &schema,
                            fragment_to_path: &fragment_to_path,
                            fragment_to_import: &fragment_to_import,
                            all_fragments: &all_fragments,
                            current_file_path: &abs_path,
                            scalars: &cfg.scalars,
                            schema_import: &schema_import,
                        };

                        let g_start = Instant::now();
                        if let Ok(_ts_code) = graphql_rust::features::codegen::generate_typescript(&doc, &ctx) {
                            ts_gen_time += g_start.elapsed();
                            total_operations += doc.graphql_trees.len(); 
                            total_fragments_processed += doc.fragments().len();
                        }
                    }
                }
            }
        }
    } else {
         // Fallback for no config
        let s_start = Instant::now();
        let schema_text = std::fs::read_to_string(_schema_path).unwrap_or_default();
        if let Ok(schema) = Schema::parse(&schema_text, _schema_path) {
            let valid_schema = match schema.clone().validate() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Schema validation failed for fallback schema: {}", e);
                    return;
                }
            };
            schema_parse_time += s_start.elapsed();

            let fr_start = Instant::now();
            let mut all_fragments = HashMap::new();
            let all_graphql_files: Vec<_> = scan_results
                .iter()
                .flatten()
                .filter(|(has_gql, _, _, _)| *has_gql)
                .map(|(_, _, _, path)| path.clone())
                .collect();

            let fragment_results: Vec<_> = all_graphql_files
                .par_iter()
                .map(|path_str| {
                    let content = std::fs::read_to_string(path_str).unwrap_or_default();
                    let abs_path = std::fs::canonicalize(path_str).unwrap_or_else(|_| std::path::PathBuf::from(path_str));
                    let uri = Url::from_file_path(&abs_path).unwrap();
                    let language = DocumentLanguage::from_uri(&uri);
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&language.get_parser_language()).unwrap();
                    let doc = DocumentState::new(uri, &content, parser);
                    
                    let mut frags = Vec::new();
                    for block in doc.get_graphql_trees() {
                        let block_text = doc.get_node_text(block.tree.root_node(), block.offset);
                        let masked = graphql_rust::utils::mask_interpolations(&block_text);
                        if let Ok(exec_doc) = apollo_compiler::executable::ExecutableDocument::parse(&valid_schema, &masked, "doc.graphql") {
                            for (name, frag) in exec_doc.fragments {
                                frags.push((name.to_string(), frag.clone()));
                            }
                        }
                    }
                    frags
                })
                .collect();

            for frags in fragment_results {
                for (name, frag) in frags {
                    all_fragments.insert(name, frag);
                }
            }
            fragment_resolve_time += fr_start.elapsed();

            let paths = graphql_rust::utils::get_project_files(scan_path);
            for path in paths {
                if is_relevant_file(&path) {
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    let abs_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    let uri = Url::from_file_path(&abs_path).unwrap();
                    let language = DocumentLanguage::from_uri(&uri);
                    
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&language.get_parser_language()).unwrap();
                    let doc = DocumentState::new(uri, &content, parser);
                    
                    if !doc.get_graphql_trees().is_empty() {
                        let ctx = graphql_rust::features::codegen::CodegenContext {
                            schema: &schema,
                            fragment_to_path: &fragment_to_path,
                            fragment_to_import: &HashMap::new(),
                            all_fragments: &all_fragments,
                            current_file_path: &abs_path,
                            scalars: &None,
                            schema_import: &None,
                        };

                        let g_start = Instant::now();
                        if let Ok(_ts_code) = graphql_rust::features::codegen::generate_typescript(&doc, &ctx) {
                            ts_gen_time += g_start.elapsed();
                            total_operations += doc.graphql_trees.len(); 
                            total_fragments_processed += doc.fragments().len();
                        }
                    }
                }
            }
        }
    }
    let total_duration = total_start.elapsed();
    let processing_duration = processing_start.elapsed();

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
