use criterion::{Criterion, criterion_group, criterion_main};
use graphox::document::DocumentState;
use std::sync::Arc;
use std::time::Duration;
use tower_lsp::lsp_types::{PositionEncodingKind, Url};
use tree_sitter::Parser;

fn generate_large_document() -> String {
    let mut doc = String::new();

    for i in 0..20 {
        doc.push_str(&format!("fragment Frag{} on Type{} {{\n", i, i % 10));
        for j in 0..20 {
            doc.push_str(&format!("  field_{}: String\n", j));
        }
        doc.push_str("}\n\n");
    }

    for i in 0..10 {
        doc.push_str(&format!("query Query{} {{\n", i));
        doc.push_str("  ...Frag0\n");
        doc.push_str("  ...Frag1\n");
        doc.push_str("  getType0 {\n");
        for j in 0..15 {
            doc.push_str(&format!("    field_{}: String\n", j));
        }
        doc.push_str("  }\n");
        doc.push_str("}\n\n");
    }

    doc
}

fn bench_arc_operations(c: &mut Criterion) {
    let content = generate_large_document();
    let uri = Url::parse("file:///test.graphql").unwrap();

    let mut group = c.benchmark_group("Arc Operations (Document Editing Investigation)");
    group.sample_size(50);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(Duration::from_millis(500));

    group.bench_function("Arc::strong_count (single reference)", |b| {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);

        b.iter(|| {
            let _count = Arc::strong_count(&doc.tree);
        })
    });

    group.bench_function("Arc::strong_count (shared reference)", |b| {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);
        let shared = doc.tree.clone();

        b.iter(|| {
            let _count = Arc::strong_count(&shared);
        })
    });

    group.bench_function("Arc::get_mut (exclusive access)", |b| {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);

        b.iter(|| {
            let mut doc = doc.clone();
            let _result = Arc::get_mut(&mut doc.tree);
        })
    });

    group.bench_function("Arc::get_mut (shared, will fail)", |b| {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);
        let shared = doc.tree.clone();

        b.iter(|| {
            let mut doc = doc.clone();
            doc.tree = shared.clone();
            let _result = Arc::get_mut(&mut doc.tree);
        })
    });

    group.finish();
}

fn bench_clone_then_edit(c: &mut Criterion) {
    let content = generate_large_document();
    let uri = Url::parse("file:///test.graphql").unwrap();

    let mut group = c.benchmark_group("Clone + Edit (Potential Optimization)");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(1000));

    group.bench_function("Clone Arc<Tree> then get_mut", |b| {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);

        b.iter(|| {
            let mut doc = doc.clone();
            // Clone Arc - now we have exclusive access
            let _tree = doc.tree.clone();
            let _result = Arc::get_mut(&mut doc.tree);
        })
    });

    group.bench_function("Full reparse (current slow path)", |b| {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        let doc = DocumentState::new(uri.clone(), &content, parser, PositionEncodingKind::UTF8);
        let shared = doc.tree.clone();

        b.iter(|| {
            let mut doc = doc.clone();
            doc.tree = shared.clone();
            if Arc::get_mut(&mut doc.tree).is_none() {
                // Current slow path: full reparse
                let full_text = doc.rope.to_string();
                let mut p = Parser::new();
                p.set_language(&tree_sitter_graphql::LANGUAGE.into())
                    .unwrap();
                doc.tree = Arc::new(p.parse(&full_text, None).unwrap());
            }
        })
    });

    group.finish();
}

fn bench_tree_sitter_parse(c: &mut Criterion) {
    let content = generate_large_document();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let mut group = c.benchmark_group("Tree-sitter Parse Operations");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_millis(1000));

    group.bench_function("Full parse (large document)", |b| {
        b.iter(|| {
            let _tree = parser.parse(&content, None).unwrap();
        })
    });

    group.bench_function("to_sexp (for clone investigation)", |b| {
        let tree = parser.parse(&content, None).unwrap();

        b.iter(|| {
            let _sexp = tree.root_node().to_sexp();
        })
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_millis(1000));
    targets = bench_arc_operations, bench_clone_then_edit, bench_tree_sitter_parse
);
criterion_main!(benches);
