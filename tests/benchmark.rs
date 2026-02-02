use apollo_compiler::Schema;
use graphql_rust::{DocumentLanguage, DocumentState};
use std::time::Instant;
use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent, Url};

#[test]
fn benchmark_document_processing() {
    let ts_content = std::fs::read_to_string("tests/fixtures/component.ts").unwrap();
    let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql").unwrap();
    let schema = Schema::parse(&schema_content, "schema.graphql").unwrap();
    let uri = Url::parse("file:///tests/fixtures/component.ts").unwrap();

    // 1. Benchmark Document Creation (Parsing)
    let start = Instant::now();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();

    // We create the document state. In the real LSP, this happens in did_open
    let doc = DocumentState::new(uri.clone(), &ts_content, parser);
    let duration = start.elapsed();
    println!("Document creation (Parsing TS): {:?}", duration);

    // 2. Benchmark GraphQL Extraction and Parsing
    let start = Instant::now();
    let trees = doc.get_graphql_trees();
    let duration = start.elapsed();
    println!(
        "Get GraphQL Trees (Extraction + Parsing): {:?} - found {} trees",
        duration,
        trees.len()
    );
    assert_eq!(trees.len(), 3);

    // 3. Benchmark Semantic Tokens
    let start = Instant::now();
    let tokens = doc.get_semantic_tokens();
    println!(
        "Large Document (100x) semantic tokens: {:?} (found {})",
        start.elapsed(),
        tokens.len()
    );

    // 4. Benchmark Diagnostics
    let start = Instant::now();
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[]);
    let duration = start.elapsed();
    println!(
        "Get Diagnostics: {:?} - found {} diagnostics",
        duration,
        diagnostics.len()
    );
}

#[test]
fn benchmark_multi_file_update() {
    let base_content = std::fs::read_to_string("tests/fixtures/component.ts").unwrap();
    let mut documents = Vec::new();

    // 1. Create 100 documents
    for i in 0..100 {
        let uri = Url::parse(&format!("file:///doc_{}.ts", i)).unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let doc = DocumentState::new(uri, &base_content, parser);
        documents.push(doc);
    }

    // 2. Initial fragment collection (simulating Backend::completion)
    let start = Instant::now();
    let mut all_fragments = Vec::new();
    for doc in &documents {
        all_fragments.extend(doc.fragments().to_vec());
    }
    let duration = start.elapsed();
    println!(
        "Initial fragment collection (100 files, cached): {:?}",
        duration
    );

    // 3. Update one document
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();

    let start_update = Instant::now();
    let doc_to_update = &mut documents[50];
    let change = TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position::new(0, 0),
            end: Position::new(0, 0),
        }),
        range_length: None,
        text: "// Updated content\n".to_string(),
    };
    doc_to_update.apply_change(&change, &mut parser);
    println!("Single document update time: {:?}", start_update.elapsed());

    // 4. Collect again
    let start = Instant::now();
    let mut all_fragments_updated = Vec::new();
    for doc in &documents {
        all_fragments_updated.extend(doc.fragments().to_vec());
    }
    let duration_updated = start.elapsed();
    println!(
        "Subsequent fragment collection (100 files, 1 updated): {:?}",
        duration_updated
    );

    assert_eq!(all_fragments.len(), all_fragments_updated.len());
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
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let doc = DocumentState::new(uri, &large_content, parser);
    println!("Large Document (100x) creation: {:?}", start.elapsed());

    let start = Instant::now();
    let trees = doc.get_graphql_trees();
    println!(
        "Large Document (100x) extract trees: {:?} (found {})",
        start.elapsed(),
        trees.len()
    );

    let start = Instant::now();
    let tokens = doc.get_semantic_tokens();
    println!(
        "Large Document (100x) semantic tokens: {:?} (found {})",
        start.elapsed(),
        tokens.len()
    );
}
