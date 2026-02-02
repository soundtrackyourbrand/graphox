use std::time::Instant;
use tower_lsp::lsp_types::{Url, Position};
use graphql_rust::{DocumentState, DocumentLanguage};
use apollo_compiler::Schema;

#[test]
fn benchmark_document_processing() {
    let ts_content = std::fs::read_to_string("tests/fixtures/component.ts").unwrap();
    let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql").unwrap();
    let schema = Schema::parse(&schema_content, "schema.graphql").unwrap();
    let uri = Url::parse("file:///tests/fixtures/component.ts").unwrap();

    // 1. Benchmark Document Creation (Parsing)
    let start = Instant::now();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()).unwrap();
    
    // We create the document state. In the real LSP, this happens in did_open
    let doc = DocumentState::new(uri.clone(), &ts_content, parser);
    let duration = start.elapsed();
    println!("Document creation (Parsing TS): {:?}", duration);

    // 2. Benchmark GraphQL Extraction and Parsing
    let start = Instant::now();
    let trees = doc.get_graphql_trees();
    let duration = start.elapsed();
    println!("Get GraphQL Trees (Extraction + Parsing): {:?} - found {} trees", duration, trees.len());
    assert_eq!(trees.len(), 3);

    // 3. Benchmark Semantic Tokens
    let start = Instant::now();
    let tokens = doc.get_semantic_tokens();
    let duration = start.elapsed();
    println!("Get Semantic Tokens: {:?} - found {} tokens", duration, tokens.len());
    assert!(!tokens.is_empty());

    // 4. Benchmark Diagnostics
    let start = Instant::now();
    let diagnostics = doc.get_semantic_diagnostics(&schema);
    let duration = start.elapsed();
    println!("Get Diagnostics: {:?} - found {} diagnostics", duration, diagnostics.len());

    // 5. Benchmark Hover (simulate hovering over 'User' in the last query)
    // Line 28:         ... on User {
    // 8 spaces + "... on " (7) = 15. 'User' starts at 15.
    let position = Position::new(28, 16); 
    let start = Instant::now();
    let hover = doc.get_hover_info(position, &schema);
    let duration = start.elapsed();
    println!("Get Hover Info: {:?}", duration);
    assert!(hover.is_some());
}

#[test]
fn benchmark_large_file_simulation() {
    // Generate a large file content by repeating the component content
    let base_content = std::fs::read_to_string("tests/fixtures/component.ts").unwrap();
    let mut large_content = String::new();
    for _ in 0..100 {
        large_content.push_str(&base_content);
        large_content.push('\n');
    }
    
    let uri = Url::parse("file:///tests/fixtures/large_component.ts").unwrap();
    
    let start = Instant::now();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()).unwrap();
    let doc = DocumentState::new(uri, &large_content, parser);
    println!("Large Document (100x) creation: {:?}", start.elapsed());

    let start = Instant::now();
    let trees = doc.get_graphql_trees();
    println!("Large Document (100x) extract trees: {:?} (found {})", start.elapsed(), trees.len());

    let start = Instant::now();
    let tokens = doc.get_semantic_tokens();
    println!("Large Document (100x) semantic tokens: {:?} (found {})", start.elapsed(), tokens.len());
}
