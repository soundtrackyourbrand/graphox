use criterion::{criterion_group, criterion_main, Criterion};
use graphox::document::DocumentState;
use std::fs;
use std::path::Path;
use tempfile::tempdir;
use tower_lsp::lsp_types::{
    Position, PositionEncodingKind, Range, TextDocumentContentChangeEvent, Url,
};
use tree_sitter::Parser;

fn generate_base_document() -> String {
    String::from(
        r#"query GetUser {
  user(id: "123") {
    id
    name
    email
  }
}
"#,
    )
}

fn bench_single_insert(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = Url::parse("file:///test.graphql").unwrap();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let mut doc = DocumentState::new(
        uri.clone(),
        &base_content,
        parser,
        PositionEncodingKind::UTF8,
    );

    let mut update_parser = Parser::new();
    update_parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(1, 2), Position::new(1, 2))),
        range_length: None,
        text: "newField: String, ".to_string(),
    };

    let mut group = c.benchmark_group("Document Editing - Single Insert");
    group.sample_size(20);
    group.bench_function("Insert field at cursor (position 1:2)", |b| {
        b.iter(|| {
            doc.apply_change(&change, &mut update_parser, doc.version + 1);
        })
    });
    group.finish();
}

fn bench_multiline_insert(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = Url::parse("file:///test.graphql").unwrap();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let mut doc = DocumentState::new(
        uri.clone(),
        &base_content,
        parser,
        PositionEncodingKind::UTF8,
    );

    let mut update_parser = Parser::new();
    update_parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(3, 0), Position::new(3, 0))),
        range_length: None,
        text: "    address {\n      street\n      city\n    }\n  ".to_string(),
    };

    let mut group = c.benchmark_group("Document Editing - Multi-line Insert");
    group.sample_size(20);
    group.bench_function("Insert address block (paste operation)", |b| {
        b.iter(|| {
            doc.apply_change(&change, &mut update_parser, doc.version + 1);
        })
    });
    group.finish();
}

fn bench_fragment_spread_add(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = Url::parse("file:///test.graphql").unwrap();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let mut doc = DocumentState::new(
        uri.clone(),
        &base_content,
        parser,
        PositionEncodingKind::UTF8,
    );

    let mut update_parser = Parser::new();
    update_parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(2, 4), Position::new(2, 4))),
        range_length: None,
        text: "...UserFields\n    ".to_string(),
    };

    let mut group = c.benchmark_group("Document Editing - Fragment Spread");
    group.sample_size(20);
    group.bench_function("Add fragment spread to selection", |b| {
        b.iter(|| {
            doc.apply_change(&change, &mut update_parser, doc.version + 1);
        })
    });
    group.finish();
}

fn bench_type_annotation_add(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = Url::parse("file:///test.graphql").unwrap();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let mut doc = DocumentState::new(
        uri.clone(),
        &base_content,
        parser,
        PositionEncodingKind::UTF8,
    );

    let mut update_parser = Parser::new();
    update_parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(2, 10), Position::new(2, 10))),
        range_length: None,
        text: "firstName: String! ".to_string(),
    };

    let mut group = c.benchmark_group("Document Editing - Type Annotation");
    group.sample_size(20);
    group.bench_function("Add non-null type annotation", |b| {
        b.iter(|| {
            doc.apply_change(&change, &mut update_parser, doc.version + 1);
        })
    });
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_single_insert, bench_multiline_insert, bench_fragment_spread_add, bench_type_annotation_add
);
criterion_main!(benches);
