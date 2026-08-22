use apollo_compiler::Schema;
use criterion::{Criterion, criterion_group, criterion_main};
use graphox::{
    DocumentState,
    features::{diagnostics::DocumentDiagnostics, semantic_tokens::DocumentSemanticTokens},
};
use std::time::Duration;
use tower_lsp_server::ls_types::{
    Position, PositionEncodingKind, Range, TextDocumentContentChangeEvent, Uri,
};

fn generate_large_schema(types_count: usize) -> String {
    let mut schema = String::from("type Query { ");
    for i in 0..types_count {
        schema.push_str(&format!("user{}: User{} ", i, i));
    }
    schema.push_str("}\n");

    for i in 0..types_count {
        schema.push_str(&format!(
            "type User{} {{ id: ID! name: String posts: [Post{}] }}\n",
            i, i
        ));
        schema.push_str(&format!(
            "type Post{} {{ id: ID! title: String author: User{} }}\n",
            i, i
        ));
    }
    schema
}

#[allow(clippy::result_large_err)]
fn bench_large_schema_parsing(c: &mut Criterion) {
    let schema_100 = generate_large_schema(100);
    let schema_1000 = generate_large_schema(1000);

    let mut group = c.benchmark_group("Large Schema Parsing");
    group.sample_size(10);
    group.bench_function("Parse Schema (100 types)", |b| {
        b.iter(|| Schema::parse(&schema_100, "schema.graphql"))
    });
    group.bench_function("Parse Schema (1000 types)", |b| {
        b.iter(|| Schema::parse(&schema_1000, "schema.graphql"))
    });
    group.finish();
}

fn bench_document_processing(c: &mut Criterion) {
    let ts_content = std::fs::read_to_string("tests/fixtures/component.ts")
        .expect("Failed to read component.ts");
    let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql")
        .expect("Failed to read simple_schema.graphql");
    let schema = Schema::parse(&schema_content, "schema.graphql")
        .expect("Failed to parse schema")
        .validate()
        .expect("Failed to validate schema");
    let uri = Uri::from_str("file:///tests/fixtures/component.ts").unwrap();

    let mut group = c.benchmark_group("Document Processing");
    group.sample_size(10);

    group.bench_function("Document creation (Parsing TS)", |b| {
        b.iter(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                .unwrap();
            DocumentState::new(
                uri.clone(),
                &ts_content,
                &mut parser,
                PositionEncodingKind::UTF8,
            )
        })
    });

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let doc = DocumentState::new(
        uri.clone(),
        &ts_content,
        &mut parser,
        PositionEncodingKind::UTF8,
    );

    group.bench_function("Get Semantic Tokens", |b| {
        b.iter(|| doc.get_semantic_tokens())
    });

    group.bench_function("Get Diagnostics", |b| {
        b.iter(|| doc.get_semantic_diagnostics(&schema, &[], None, None, false, true))
    });

    group.finish();
}

fn bench_multi_file_update(c: &mut Criterion) {
    let base_content = std::fs::read_to_string("tests/fixtures/component.ts")
        .expect("Failed to read component.ts");

    let mut group = c.benchmark_group("Multi-file Updates");
    group.sample_size(10);

    group.bench_function("Initial fragment collection (100 files)", |b| {
        b.iter_with_setup(
            || {
                let mut documents = Vec::new();
                for i in 0..100 {
                    let uri = Uri::from_str(&format!("file:///doc_{}.ts", i)).unwrap();
                    let mut parser = tree_sitter::Parser::new();
                    parser
                        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                        .unwrap();
                    let doc = DocumentState::new(
                        uri,
                        &base_content,
                        &mut parser,
                        PositionEncodingKind::UTF8,
                    );
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
        let uri = Uri::from_str("file:///doc_50.ts").unwrap();
        let mut doc =
            DocumentState::new(uri, &base_content, &mut parser, PositionEncodingKind::UTF8);
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
            doc.apply_change(&change, &mut update_parser, 0);
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
    let uri = Uri::from_str("file:///tests/fixtures/large_component.ts").unwrap();

    let mut group = c.benchmark_group("Large File Simulation (100x)");
    group.sample_size(10);

    group.bench_function("Large Document creation", |b| {
        b.iter(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                .unwrap();
            DocumentState::new(
                uri.clone(),
                &large_content,
                &mut parser,
                PositionEncodingKind::UTF8,
            )
        })
    });

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let doc = DocumentState::new(
        uri.clone(),
        &large_content,
        &mut parser,
        PositionEncodingKind::UTF8,
    );

    group.bench_function("Large Document extract trees", |b| {
        b.iter(|| doc.get_graphql_trees())
    });

    group.bench_function("Large Document semantic tokens", |b| {
        b.iter(|| doc.get_semantic_tokens())
    });

    group.finish();
}

fn bench_fragment_heavy_document(c: &mut Criterion) {
    let mut text = String::from("query Heavy { users { id ...F0 }\n");
    for i in 0..99 {
        text.push_str(&format!("...F{} ", i + 1));
    }
    text.push_str("} }\n");

    for i in 0..100 {
        text.push_str(&format!("fragment F{} on User {{ username email }}\n", i));
    }

    let uri = Uri::from_str("file:///heavy.graphql").unwrap();
    let mut group = c.benchmark_group("Fragment Heavy Document");
    group.sample_size(10);

    group.bench_function("Parse and Extract Fragments (100 fragments)", |b| {
        b.iter(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_graphql::LANGUAGE.into())
                .unwrap();
            DocumentState::new(uri.clone(), &text, &mut parser, PositionEncodingKind::UTF8)
        })
    });

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    let doc = DocumentState::new(uri.clone(), &text, &mut parser, PositionEncodingKind::UTF8);

    group.bench_function("Get Fragments Info", |b| b.iter(|| doc.fragments()));

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_millis(800)).measurement_time(Duration::from_millis(1000));
    targets =
        bench_document_processing,
        bench_multi_file_update,
        bench_large_file_simulation,
        bench_large_schema_parsing,
        bench_fragment_heavy_document
);
criterion_main!(benches);
