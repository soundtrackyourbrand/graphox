use apollo_compiler::Schema;
use graphql_rust::utils::is_relevant_file;
use graphql_rust::{Config, DocumentLanguage, DocumentState};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tower_lsp::lsp_types::Url;

pub async fn run_benchmark(mut config: Option<Config>, schema_path: &str, scan_path: &str) {
    if config.is_none() {
        config = Config::load_from_dir(scan_path);
    }
    
    println!("Starting Benchmark...");
    let total_start = Instant::now();

    // Timings for Scan Phase
    let mut file_discovery_time = Duration::ZERO;
    let mut file_io_time = Duration::ZERO;
    let mut ts_parsing_time = Duration::ZERO;
    let mut gql_extraction_time = Duration::ZERO;
    let mut fragment_extraction_time = Duration::ZERO;
    let mut path_canonicalize_time = Duration::ZERO;

    // 1. Initial Scan & Fragment Collection
    let mut fragment_to_path = HashMap::new();
    let mut fragment_to_import = HashMap::new();
    let mut total_graphql_files = 0;

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
    file_discovery_time += discovery_start.elapsed();

    let total_files_scanned = scan_root.len();

    for (path, import_alias) in &scan_root {
        let io_start = Instant::now();
        let content = std::fs::read_to_string(path).unwrap_or_default();
        file_io_time += io_start.elapsed();

        let canon_start = Instant::now();
        let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        path_canonicalize_time += canon_start.elapsed();
        
        let uri = Url::from_file_path(&abs_path).unwrap();
        let language = DocumentLanguage::from_uri(&uri);
        
        let ts_start = Instant::now();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.get_parser_language())
            .unwrap();
        let rope = ropey::Rope::from_str(&content);
        let tree = parser.parse(&content, None).unwrap();
        ts_parsing_time += ts_start.elapsed();

        let mut doc = DocumentState {
            uri,
            rope,
            tree,
            language,
            graphql_trees: Vec::new(),
            fragments: Vec::new(),
            package_root: None,
        };

        let gql_start = Instant::now();
        doc.graphql_trees = doc.reparse_graphql_trees();
        gql_extraction_time += gql_start.elapsed();

        if !doc.graphql_trees.is_empty() {
            total_graphql_files += 1;
            let frag_start = Instant::now();
            doc.fragments = doc.extract_fragment_names();
            for frag in &doc.fragments {
                fragment_to_path.insert(frag.name.clone(), abs_path.to_string_lossy().to_string());
                if let Some(alias) = import_alias {
                    fragment_to_import.insert(frag.name.clone(), alias.clone());
                }
            }
            fragment_extraction_time += frag_start.elapsed();
        }
    }

    // 2. Project Processing
    let mut total_operations = 0;
    let mut total_fragments = 0;
    let mut schema_parse_time = Duration::ZERO;
    let mut ts_gen_time = Duration::ZERO;
    let mut file_write_time = Duration::ZERO;

    if let Some(cfg) = &config {
        for project in &cfg.projects {
            let discovery_start = Instant::now();
            let abs_include = cfg.base_dir.join(&project.include).to_string_lossy().to_string();
            let paths = graphql_rust::utils::get_project_files(&abs_include);
            file_discovery_time += discovery_start.elapsed();
            
            // Schema loading
            let s_start = Instant::now();
            let mut combined_text = String::new();
            for file in project.schema.files() {
                let io_start = Instant::now();
                if let Ok(t) = std::fs::read_to_string(cfg.base_dir.join(file)) {
                    combined_text.push_str(&t);
                    combined_text.push('\n');
                }
                file_io_time += io_start.elapsed();
            }
            let schema = Schema::parse(&combined_text, &project.schema.as_key()).unwrap();
            schema_parse_time += s_start.elapsed();

            for path in paths {
                if is_relevant_file(&path) {
                        let io_start = Instant::now();
                        let content = std::fs::read_to_string(&path).unwrap_or_default();
                        file_io_time += io_start.elapsed();

                        let canon_start = Instant::now();
                        let abs_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                        path_canonicalize_time += canon_start.elapsed();


                    let uri = Url::from_file_path(&abs_path).unwrap();
                    let language = DocumentLanguage::from_uri(&uri);
                    
                    let ts_start = Instant::now();
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&language.get_parser_language()).unwrap();
                    let rope = ropey::Rope::from_str(&content);
                    let tree = parser.parse(&content, None).unwrap();
                    ts_parsing_time += ts_start.elapsed();

                    let mut doc = DocumentState {
                        uri,
                        rope,
                        tree,
                        language,
                        graphql_trees: Vec::new(),
                        fragments: Vec::new(),
                        package_root: None,
                    };

                    let gql_start = Instant::now();
                    doc.graphql_trees = doc.reparse_graphql_trees();
                    gql_extraction_time += gql_start.elapsed();

                    if !doc.graphql_trees.is_empty() {
                        let schema_import = cfg.schema_types.as_ref().and_then(|sts| {
                            sts.iter()
                                .find(|st| st.schema.as_key() == project.schema.as_key())
                                .and_then(|st| st.import.clone())
                        });

                        let ctx = graphql_rust::features::codegen::CodegenContext {
                            schema: &schema,
                            fragment_to_path: &fragment_to_path,
                            fragment_to_import: &fragment_to_import,
                            current_file_path: &abs_path,
                            scalars: &cfg.scalars,
                            schema_import: &schema_import,
                        };

                        let g_start = Instant::now();
                        if let Ok(_ts_code) = graphql_rust::features::codegen::generate_typescript(&doc, &ctx) {
                            ts_gen_time += g_start.elapsed();
                            
                            // Mock file writing for benchmark
                            let w_start = Instant::now();
                            file_write_time += w_start.elapsed();

                            // Track stats
                            total_operations += doc.graphql_trees.len(); 
                            
                            let frag_start = Instant::now();
                            doc.fragments = doc.extract_fragment_names();
                            total_fragments += doc.fragments.len();
                            fragment_extraction_time += frag_start.elapsed();
                        }
                    }
                }
            }
        }
    } else {
        // Simple case without config
        let s_start = Instant::now();
        let io_start = Instant::now();
        let schema_text = std::fs::read_to_string(schema_path);
        file_io_time += io_start.elapsed();

        if let Ok(text) = schema_text {
             if let Ok(schema) = Schema::parse(&text, schema_path) {
                schema_parse_time += s_start.elapsed();

                let discovery_start = Instant::now();
                let paths = graphql_rust::utils::get_project_files(scan_path);
                file_discovery_time += discovery_start.elapsed();

                for path in paths {
                    if is_relevant_file(&path) {
                        let io_start = Instant::now();
                        let content = std::fs::read_to_string(&path).unwrap_or_default();
                        file_io_time += io_start.elapsed();

                        let canon_start = Instant::now();
                        let abs_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                        path_canonicalize_time += canon_start.elapsed();

                        let uri = Url::from_file_path(&abs_path).unwrap();
                        let language = DocumentLanguage::from_uri(&uri);
                        
                        let ts_start = Instant::now();
                        let mut parser = tree_sitter::Parser::new();
                        parser.set_language(&language.get_parser_language()).unwrap();
                        let rope = ropey::Rope::from_str(&content);
                        let tree = parser.parse(&content, None).unwrap();
                        ts_parsing_time += ts_start.elapsed();

                        let mut doc = DocumentState {
                            uri,
                            rope,
                            tree,
                            language,
                            graphql_trees: Vec::new(),
                            fragments: Vec::new(),
                            package_root: None,
                        };

                        let gql_start = Instant::now();
                        doc.graphql_trees = doc.reparse_graphql_trees();
                        gql_extraction_time += gql_start.elapsed();

                        if !doc.graphql_trees.is_empty() {
                            let ctx = graphql_rust::features::codegen::CodegenContext {
                                schema: &schema,
                                fragment_to_path: &fragment_to_path,
                                fragment_to_import: &HashMap::new(),
                                current_file_path: &abs_path,
                                scalars: &None,
                                schema_import: &None,
                            };

                            let g_start = Instant::now();
                            if let Ok(_ts_code) = graphql_rust::features::codegen::generate_typescript(&doc, &ctx) {
                                ts_gen_time += g_start.elapsed();
                                total_operations += doc.graphql_trees.len();
                                
                                let frag_start = Instant::now();
                                doc.fragments = doc.extract_fragment_names();
                                total_fragments += doc.fragments.len();
                                fragment_extraction_time += frag_start.elapsed();
                            }
                        }
                    }
                }
             } else {
                 eprintln!("Failed to parse schema: {}", schema_path);
             }
        } else {
            eprintln!("Failed to read schema: {}", schema_path);
        }
    }
    let total_duration = total_start.elapsed();

    println!("\n--- Benchmark Results ---");
    println!("Total Files Scanned:      {}", total_files_scanned);
    println!("Files with GraphQL:       {}", total_graphql_files);
    println!("Total Fragments Found:    {}", total_fragments);
    println!("Total Operations:         {}", total_operations);
    println!("");
    println!("Phase Timings:");
    println!("  File Discovery:         {:>10?}", file_discovery_time);
    println!("  File IO:                {:>10?}", file_io_time);
    println!("  Path Canonicalize:      {:>10?}", path_canonicalize_time);
    println!("  TS Parsing (TreeSitter):{:>10?}", ts_parsing_time);
    println!("  GQL Extraction (Query): {:>10?}", gql_extraction_time);
    println!("  Fragment Extraction:    {:>10?}", fragment_extraction_time);
    println!("  Schema Parsing:         {:>10?}", schema_parse_time);
    println!("  TS Generation:          {:>10?}", ts_gen_time);
    println!("  File Writing (est):     {:>10?}", file_write_time);
    println!("--------------------------");
    println!("Total Wall Time:          {:>10?}", total_duration);
}
