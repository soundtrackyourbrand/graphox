use apollo_compiler::Schema;
use graphql_rust::utils::is_relevant_file;
use graphql_rust::{DocumentLanguage, DocumentState};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub async fn run_codegen(
    schema_path: &str,
    scan_path: &str,
    output_dir: Option<&str>,
    watch: bool,
) {
    if !watch {
        execute_codegen(schema_path, scan_path, output_dir).await;
        return;
    }

    println!("Watching for changes in {}...", scan_path);
    execute_codegen(schema_path, scan_path, output_dir).await;

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

    debouncer
        .watcher()
        .watch(Path::new(schema_path), notify::RecursiveMode::NonRecursive)
        .expect("Failed to watch schema");

    while let Some(_) = rx.recv().await {
        println!("\nChange detected, re-running codegen...");
        execute_codegen(schema_path, scan_path, output_dir).await;
    }
}

async fn execute_codegen(schema_path: &str, scan_path: &str, output_dir: Option<&str>) {
    let schema_text = std::fs::read_to_string(schema_path).expect("Failed to read schema");
    let schema = Schema::parse(&schema_text, schema_path).expect("Failed to parse schema");

    let mut docs = Vec::new();

    for entry in WalkBuilder::new(scan_path)
        .add_custom_ignore_filename(".graphqlignore")
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
    {
        let path = entry.path().to_owned();
        if is_relevant_file(&path) {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let uri = Url::from_file_path(std::fs::canonicalize(&path).unwrap()).unwrap();
            let language = DocumentLanguage::from_uri(&uri);
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&language.get_parser_language())
                .unwrap();
            let doc = DocumentState::new(uri, &content, parser);
            if !doc.get_graphql_trees().is_empty() {
                docs.push((path, doc));
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
            current_file_path: &path,
        };

        match graphql_rust::features::codegen::generate_typescript(doc, &ctx) {
            Ok(ts_code) => {
                let out_path = if let Some(dir) = output_dir {
                    let rel = path.strip_prefix(scan_path).unwrap_or(&path);
                    let mut p = PathBuf::from(dir);
                    p.push(rel);
                    p.set_extension("graphql.codegen.ts");
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
