use apollo_compiler::Schema;
use graphql_rust::utils::is_relevant_file;
use graphql_rust::{Config, DocumentLanguage, DocumentState};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub async fn run_codegen(
    config: Option<Config>,
    schema_path: &str,
    scan_path: &str,
    output_dir: Option<&str>,
    watch: bool,
) {
    if !watch {
        execute_codegen(config, schema_path, scan_path, output_dir).await;
        return;
    }

    println!("Watching for changes...");
    execute_codegen(config.clone(), schema_path, scan_path, output_dir).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    let mut debouncer = notify_debouncer_mini::new_debouncer(
        std::time::Duration::from_millis(200),
        move |res: notify_debouncer_mini::DebounceEventResult| match res {
            Ok(_) => {
                let _ = tx.blocking_send(());
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        },
    )
    .expect("Failed to create debouncer");

    debouncer
        .watcher()
        .watch(Path::new(scan_path), notify::RecursiveMode::Recursive)
        .expect("Failed to watch directory");

    if let Some(cfg) = &config {
        for project in &cfg.projects {
            for file in project.schema.files() {
                debouncer
                    .watcher()
                    .watch(&cfg.base_dir.join(file), notify::RecursiveMode::NonRecursive)
                    .ok();
            }
        }
        if let Some(schema_types) = &cfg.schema_types {
            for st in schema_types {
                for file in st.schema.files() {
                    debouncer
                        .watcher()
                        .watch(&cfg.base_dir.join(file), notify::RecursiveMode::NonRecursive)
                        .ok();
                }
            }
        }
    } else {
        debouncer
            .watcher()
            .watch(Path::new(schema_path), notify::RecursiveMode::NonRecursive)
            .expect("Failed to watch schema");
    }

    while rx.recv().await.is_some() {
        println!("\nChange detected, re-running codegen...");
        execute_codegen(config.clone(), schema_path, scan_path, output_dir).await;
    }
}

async fn execute_codegen(
    config: Option<Config>,
    schema_path: &str,
    scan_path: &str,
    output_dir: Option<&str>,
) {
    if let Some(cfg) = &config {
        use rayon::prelude::*;

        let mut fragment_to_path = HashMap::new();
        let mut fragment_to_import = HashMap::new();

        // Collect all scan roots
        let mut all_scan_roots = Vec::new();
        for project in &cfg.projects {
            let abs_include = cfg.base_dir.join(&project.include).to_string_lossy().to_string();
            all_scan_roots.push((abs_include, project.import.clone()));
        }

        // First pass: find all fragments across all projects in parallel
        let scan_results: Vec<_> = all_scan_roots
            .par_iter()
            .map(|(abs_include, import_alias)| {
                let paths = graphql_rust::utils::get_project_files(abs_include);
                let mut results = Vec::new();
                for path in paths {
                    if is_relevant_file(&path) {
                        let content = std::fs::read_to_string(&path).unwrap_or_default();
                        let abs_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                        let uri = Url::from_file_path(&abs_path).unwrap();
                        let language = DocumentLanguage::from_uri(&uri);
                        let mut parser = tree_sitter::Parser::new();
                        parser
                            .set_language(&language.get_parser_language())
                            .unwrap();
                        let doc = DocumentState::new(uri, &content, parser);
                        for frag in doc.fragments() {
                            results.push((
                                frag.name.clone(),
                                abs_path.to_string_lossy().to_string(),
                                import_alias.clone(),
                            ));
                        }
                    }
                }
                results
            })
            .collect();

        for results in &scan_results {
            for (name, path, alias) in results {
                fragment_to_path.insert(name.clone(), path.clone());
                if let Some(a) = alias {
                    fragment_to_import.insert(name.clone(), a.clone());
                }
            }
        }

        let global_output_dir = cfg.output_dir.as_deref().or(output_dir);
        for project in &cfg.projects {
            let abs_include = cfg.base_dir.join(&project.include).to_string_lossy().to_string();

            println!("Processing project with schema: {}", project.schema.as_key());
            let project_output_dir = project.output_dir.as_deref().or(global_output_dir);

            let schema_import = cfg.schema_types.as_ref().and_then(|sts| {
                sts.iter()
                    .find(|st| st.schema.as_key() == project.schema.as_key())
                    .and_then(|st| st.import.clone())
            });

            // Re-parse all fragments for THIS project's schema to get executable::Fragment objects
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
            let valid_schema = match schema.validate() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Schema validation failed for {}: {}", project.schema.as_key(), e);
                    continue;
                }
            };

            let mut all_fragments = HashMap::new();
            let all_graphql_files: Vec<_> = scan_results
                .iter()
                .flatten()
                .map(|(_, path, _)| path.clone())
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

            execute_project_codegen(
                &cfg.base_dir,
                &project.schema,
                &abs_include,
                project_output_dir,
                &cfg.scalars,
                &schema_import,
                &fragment_to_path,
                &fragment_to_import,
                &all_fragments,
            )
            .await;
        }

        if let Some(schema_types) = &cfg.schema_types {
            for st in schema_types {
                let abs_output = cfg.base_dir.join(&st.output);
                println!("Generating types for schema: {}", st.schema.as_key());
                execute_schema_codegen(&cfg.base_dir, &st.schema, &abs_output.to_string_lossy(), &cfg.scalars).await;
            }
        }
    } else {
        use rayon::prelude::*;
        let mut fragment_to_path = HashMap::new();
        let paths = graphql_rust::utils::get_project_files(scan_path);
        
        let results: Vec<_> = paths
            .par_iter()
            .filter(|p| is_relevant_file(p))
            .map(|path| {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                let uri = Url::from_file_path(&abs_path).unwrap();
                let language = DocumentLanguage::from_uri(&uri);
                let mut parser = tree_sitter::Parser::new();
                parser
                    .set_language(&language.get_parser_language())
                    .unwrap();
                let doc = DocumentState::new(uri, &content, parser);
                doc.fragments().iter().map(|f| (f.name.clone(), abs_path.to_string_lossy().to_string())).collect::<Vec<_>>()
            })
            .collect();

        for frags in results {
            for (name, path) in frags {
                fragment_to_path.insert(name, path);
            }
        }

        let mut all_fragments_for_fallback = HashMap::new();
        let schema_text = std::fs::read_to_string(schema_path).unwrap_or_default();
        if let Ok(schema) = Schema::parse(&schema_text, schema_path) {
            if let Ok(valid_schema) = schema.validate() {
                for path in &paths {
                    if is_relevant_file(path) {
                        let content = std::fs::read_to_string(path).unwrap_or_default();
                        let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                        let uri = Url::from_file_path(&abs_path).unwrap();
                        let language = DocumentLanguage::from_uri(&uri);
                        let mut parser = tree_sitter::Parser::new();
                        parser.set_language(&language.get_parser_language()).unwrap();
                        let doc = DocumentState::new(uri, &content, parser);
                        for block in doc.get_graphql_trees() {
                            let block_text = doc.get_node_text(block.tree.root_node(), block.offset);
                            let masked = graphql_rust::utils::mask_interpolations(&block_text);
                            if let Ok(exec_doc) = apollo_compiler::executable::ExecutableDocument::parse(&valid_schema, &masked, "doc.graphql") {
                                for (name, frag) in exec_doc.fragments {
                                    all_fragments_for_fallback.insert(name.to_string(), frag.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        execute_project_codegen(
            Path::new("."),
            &graphql_rust::config::SchemaSource::Single(schema_path.to_string()),
            scan_path,
            output_dir,
            &None,
            &None,
            &fragment_to_path,
            &HashMap::new(),
            &all_fragments_for_fallback,
        ).await;
    }
}

async fn execute_schema_codegen(
    base_dir: &Path,
    source: &graphql_rust::config::SchemaSource,
    output_path: &str,
    scalars: &Option<HashMap<String, String>>,
) {
    let mut combined_text = String::new();
    for file in source.files() {
        match std::fs::read_to_string(base_dir.join(file)) {
            Ok(t) => {
                combined_text.push_str(&t);
                combined_text.push('\n');
            }
            Err(e) => {
                eprintln!("Failed to read schema {}: {}", source.as_key(), e);
                return;
            }
        }
    }
    let schema = match apollo_compiler::Schema::parse(&combined_text, &source.as_key()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse schema {}: {}", source.as_key(), e);
            return;
        }
    };

    let ts_code = graphql_rust::features::codegen::generate_schema_types(&schema, scalars);
    let out_path = Path::new(output_path);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(out_path, ts_code).expect("Failed to write schema types file");
    println!("Generated schema types: {}", out_path.display());
}

async fn execute_project_codegen(
    base_dir: &Path,
    source: &graphql_rust::config::SchemaSource,
    include_glob: &str,
    output_dir: Option<&str>,
    scalars: &Option<HashMap<String, String>>,
    schema_import: &Option<String>,
    fragment_to_path: &HashMap<String, String>,
    fragment_to_import: &HashMap<String, String>,
    all_fragments: &HashMap<String, apollo_compiler::Node<apollo_compiler::executable::Fragment>>,
) {
    let mut combined_text = String::new();
    for file in source.files() {
        match std::fs::read_to_string(base_dir.join(file)) {
            Ok(t) => {
                combined_text.push_str(&t);
                combined_text.push('\n');
            }
            Err(e) => {
                eprintln!("Failed to read schema {}: {}", source.as_key(), e);
                return;
            }
        }
    }
    let schema = match Schema::parse(&combined_text, &source.as_key()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse schema {}: {}", source.as_key(), e);
            return;
        }
    };

    let (scan_root, paths) = if include_glob.contains('*') {
        (
            None,
            graphql_rust::utils::get_project_files(include_glob),
        )
    } else {
        let p = Path::new(include_glob);
        if p.is_dir() {
            (
                Some(std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())),
                graphql_rust::utils::get_project_files(include_glob),
            )
        } else {
            let abs_file = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            let parent = abs_file.parent().map(|pa| pa.to_path_buf());
            (parent, vec![abs_file])
        }
    };

    let mut docs = Vec::new();
    for path in paths {
        if is_relevant_file(&path) {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let abs_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let uri = Url::from_file_path(&abs_path).unwrap();
            let language = DocumentLanguage::from_uri(&uri);
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&language.get_parser_language())
                .unwrap();
            let doc = DocumentState::new(uri, &content, parser);
            if !doc.get_graphql_trees().is_empty() {
                docs.push((abs_path, doc));
            }
        }
    }

    for (path, doc) in &docs {
        let ctx = graphql_rust::features::codegen::CodegenContext {
            schema: &schema,
            fragment_to_path,
            fragment_to_import,
            all_fragments,
            current_file_path: path,
            scalars,
            schema_import,
        };

        match graphql_rust::features::codegen::generate_typescript(doc, &ctx) {
            Ok(ts_code) => {
                let out_path = if let Some(dir) = output_dir {
                    let mut p = PathBuf::from(dir);
                    let rel = if let Some(root) = &scan_root {
                        path.strip_prefix(root).unwrap_or(path)
                    } else {
                        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                        let abs_cwd =
                            std::fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from("."));
                        path.strip_prefix(&abs_cwd).unwrap_or(path)
                    };
                    p.push(rel);
                    p.set_extension("codegen.ts");
                    p
                } else {
                    let mut p = path.clone();
                    p.set_extension("codegen.ts");
                    p
                };

                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&out_path, ts_code).expect("Failed to write codegen file");
                println!("Generated: {}", out_path.display());
            }
            Err(e) => {
                eprintln!("Error generating types for {}: {}", path.display(), e);
            }
        }
    }
}
