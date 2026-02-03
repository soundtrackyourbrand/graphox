use apollo_compiler::Schema;
use criterion::{criterion_group, criterion_main, Criterion};
use graphql_rust::DocumentState;
use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent, Url};

fn bench_document_processing(c: &mut Criterion) {
    let ts_content = std::fs::read_to_string("tests/fixtures/component.ts")
        .expect("Failed to read component.ts");
    let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql")
        .expect("Failed to read simple_schema.graphql");
    let schema = Schema::parse(&schema_content, "schema.graphql").expect("Failed to parse schema");
    let uri = Url::parse("file:///tests/fixtures/component.ts").unwrap();

    let mut group = c.benchmark_group("Document Processing");

    group.bench_function("Document creation (Parsing TS)", |b| {
        b.iter(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                .unwrap();
            DocumentState::new(uri.clone(), &ts_content, parser)
        })
    });

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let doc = DocumentState::new(uri.clone(), &ts_content, parser);

    group.bench_function("Get GraphQL Trees", |b| b.iter(|| doc.get_graphql_trees()));

    group.bench_function("Get Semantic Tokens", |b| {
        b.iter(|| doc.get_semantic_tokens())
    });

    group.bench_function("Get Diagnostics", |b| {
        b.iter(|| doc.get_semantic_diagnostics(&schema, &[], None))
    });

    group.finish();
}

fn bench_multi_file_update(c: &mut Criterion) {
    let base_content = std::fs::read_to_string("tests/fixtures/component.ts")
        .expect("Failed to read component.ts");

    let mut group = c.benchmark_group("Multi-file Updates");

    group.bench_function("Initial fragment collection (100 files)", |b| {
        b.iter_with_setup(
            || {
                let mut documents = Vec::new();
                for i in 0..100 {
                    let uri = Url::parse(&format!("file:///doc_{}.ts", i)).unwrap();
                    let mut parser = tree_sitter::Parser::new();
                    parser
                        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                        .unwrap();
                    let doc = DocumentState::new(uri, &base_content, parser);
                    documents.push(doc);
                }
                documents
            },
            |documents| {
                let mut all_fragments = Vec::new();
                for doc in &documents {
                    all_fragments.extend(doc.fragments().to_vec());
                }
                all_fragments
            },
        )
    });

    group.bench_function("Single document update", |b| {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let uri = Url::parse("file:///doc_50.ts").unwrap();
        let mut doc = DocumentState::new(uri, &base_content, parser);
        let mut update_parser = tree_sitter::Parser::new();
        update_parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();

        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 0),
                end: Position::new(0, 0),
            }),
            range_length: None,
            text: "// Updated content\n".to_string(),
        };

        b.iter(|| {
            doc.apply_change(&change, &mut update_parser);
        })
    });

    group.finish();
}

fn bench_large_file_simulation(c: &mut Criterion) {
    let base_content = std::fs::read_to_string("tests/fixtures/component.ts")
        .expect("Failed to read component.ts");
    let mut large_content = String::new();
    for _ in 0..100 {
        large_content.push_str(&base_content);
        large_content.push('\n');
    }
    let uri = Url::parse("file:///tests/fixtures/large_component.ts").unwrap();

    let mut group = c.benchmark_group("Large File Simulation (100x)");

    group.bench_function("Large Document creation", |b| {
        b.iter(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                .unwrap();
            DocumentState::new(uri.clone(), &large_content, parser)
        })
    });

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let doc = DocumentState::new(uri.clone(), &large_content, parser);

    group.bench_function("Large Document extract trees", |b| {
        b.iter(|| doc.get_graphql_trees())
    });

    group.bench_function("Large Document semantic tokens", |b| {
        b.iter(|| doc.get_semantic_tokens())
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_document_processing,
    bench_multi_file_update,
    bench_large_file_simulation
);
criterion_main!(benches);
