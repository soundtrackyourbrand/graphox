//! Long running memory test with subgraphs.

use std::fs;

use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};
use tempfile::TempDir;
use tower_lsp::lsp_types::Url;

use crate::support::{
    create_large_schema, lsp_did_close, lsp_did_open, lsp_initialize_sequence, measure_memory_usage,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ntest::timeout(10000)]
async fn test_memory_long_running_subgraphs() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    // Create a large schema to exercise memory management under realistic load
    let main_schema = create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), main_schema).unwrap();

    // Generate many subgraphs to test scalability of cross-project resolution
    let subgraphs_dir = base_dir.join("subgraphs");
    fs::create_dir_all(&subgraphs_dir).unwrap();
    for i in 0..50 {
        let mut subgraph = format!("type SubgraphType{} @key(fields: \"id\") {{\n", i);
        subgraph.push_str("  id: ID!\n");
        for j in 0..20 {
            subgraph.push_str(&format!("  field{}: String\n", j));
        }
        subgraph.push_str("}\n\n");

        subgraph.push_str("extend type Query {\n");
        subgraph.push_str(&format!(
            "  subgraphNode{}(id: ID!): SubgraphType{}\n",
            i, i
        ));
        subgraph.push_str("}\n");

        fs::write(
            subgraphs_dir.join(format!("subgraph_{}.graphql", i)),
            subgraph,
        )
        .unwrap();
    }

    // Populate the workspace with many operations to test performance of background scanning
    for i in 0..100 {
        let query = format!(
            "query Query{} {{ subgraphNode{}(id: \"1\") {{ id field0 field1 }} }}",
            i,
            i % 50
        );
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let baseline = measure_memory_usage();

    // Initialize LSP with subgraph-aware configuration
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    // Initialize LSP service and await full workspace scan
    let (mut service, _) =
        tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
    lsp_initialize_sequence(&mut service).await;

    // Simulate long-running session by cycling through many files.
    // We open and then close each file to ensure we aren't just leaking open document state.
    for cycle in 0..3 {
        println!("Cycle {}...", cycle);
        for i in 0..50 {
            let file_path = base_dir.join(format!("query_{}.graphql", i + cycle * 10));
            if file_path.exists() {
                let uri = Url::from_file_path(file_path.clone()).unwrap();
                let text = fs::read_to_string(file_path).unwrap();
                lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;
                lsp_did_close(&mut service, uri).await;
            }
        }

        // Wait a bit for processing
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let used = measure_memory_usage();
        println!(
            "Cycle {} memory: {} MB",
            cycle,
            (used.saturating_sub(baseline)) / 1024 / 1024
        );
    }

    let final_used = measure_memory_usage();
    let delta = final_used.saturating_sub(baseline);

    println!(
        "Final memory for long running subgraphs: {} MB",
        delta / 1024 / 1024
    );

    // Assert some reasonable limit
    #[cfg(not(target_os = "windows"))]
    {
        assert!(
            delta < 100 * 1024 * 1024,
            "Memory exceeded limit for long running subgraphs: {} MB",
            delta / 1024 / 1024
        );
    }
}
