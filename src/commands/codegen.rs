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
            debouncer
                .watcher()
                .watch(&cfg.base_dir.join(&project.schema), notify::RecursiveMode::NonRecursive)
                .ok();
        }
        if let Some(schema_types) = &cfg.schema_types {
            for st in schema_types {
                debouncer
                    .watcher()
                    .watch(&cfg.base_dir.join(&st.schema), notify::RecursiveMode::NonRecursive)
                    .ok();
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
        let global_output_dir = cfg.output_dir.as_deref().or(output_dir);
        for project in &cfg.projects {
            let abs_schema = cfg.base_dir.join(&project.schema);
            let abs_include = if project.include.contains('*') {
                cfg.base_dir.join(&project.include).to_string_lossy().to_string()
            } else {
                cfg.base_dir.join(&project.include).to_string_lossy().to_string()
            };

            println!("Processing project with schema: {}", project.schema);
            let project_output_dir = project.output_dir.as_deref().or(global_output_dir);
            execute_project_codegen(
                &abs_schema.to_string_lossy(),
                &abs_include,
                project_output_dir,
                &cfg.scalars,
            )
            .await;
        }

        if let Some(schema_types) = &cfg.schema_types {
            for st in schema_types {
                let abs_schema = cfg.base_dir.join(&st.schema);
                let abs_output = cfg.base_dir.join(&st.output);
                println!("Generating types for schema: {}", st.schema);
                execute_schema_codegen(&abs_schema.to_string_lossy(), &abs_output.to_string_lossy(), &cfg.scalars).await;
            }
        }
    } else {
        execute_project_codegen(schema_path, scan_path, output_dir, &None).await;
    }
}

async fn execute_schema_codegen(
    schema_path: &str,
    output_path: &str,
    scalars: &Option<HashMap<String, String>>,
) {
    let schema_text = match std::fs::read_to_string(schema_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to read schema {}: {}", schema_path, e);
            return;
        }
    };
    let schema = match apollo_compiler::Schema::parse(&schema_text, schema_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse schema {}: {}", schema_path, e);
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
    schema_path: &str,
    include_glob: &str,
    output_dir: Option<&str>,
    scalars: &Option<HashMap<String, String>>,
) {
    let schema_text = match std::fs::read_to_string(schema_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to read schema {}: {}", schema_path, e);
            return;
        }
    };
    let schema = match Schema::parse(&schema_text, schema_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse schema {}: {}", schema_path, e);
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

    let mut fragment_to_path = HashMap::new();
    for (path, doc) in &docs {
        for frag in doc.fragments() {
            fragment_to_path.insert(frag.name.clone(), path.to_string_lossy().to_string());
        }
    }

    for (path, doc) in &docs {
        let ctx = graphql_rust::features::codegen::CodegenContext {
            schema: &schema,
            fragment_to_path: &fragment_to_path,
            current_file_path: path,
            scalars,
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
                    let mut filename = p.file_name().unwrap().to_os_string();
                    filename.push(".codegen.ts");
                    p.set_file_name(filename);
                    p
                } else {
                    let mut p = path.clone();
                    let mut filename = p.file_name().unwrap().to_os_string();
                    filename.push(".codegen.ts");
                    p.set_file_name(filename);
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
