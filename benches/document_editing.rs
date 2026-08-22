use criterion::{Criterion, criterion_group, criterion_main};
use graphox::document::DocumentState;
use tower_lsp_server::ls_types::{
    Position, PositionEncodingKind, Range, TextDocumentContentChangeEvent, Uri,
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
    let uri = "file:///test.graphql".parse::<Uri>().unwrap();

    let doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &base_content,
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
        b.iter_with_setup(
            || doc.clone(),
            |mut d| {
                d.apply_change(&change, &mut update_parser, d.version + 1);
            },
        )
    });
    group.finish();
}

fn bench_multiline_insert(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = "file:///test.graphql".parse::<Uri>().unwrap();

    let doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &base_content,
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
        b.iter_with_setup(
            || doc.clone(),
            |mut d| {
                d.apply_change(&change, &mut update_parser, d.version + 1);
            },
        )
    });
    group.finish();
}

fn bench_fragment_spread_add(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = "file:///test.graphql".parse::<Uri>().unwrap();

    let doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &base_content,
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
        b.iter_with_setup(
            || doc.clone(),
            |mut d| {
                d.apply_change(&change, &mut update_parser, d.version + 1);
            },
        )
    });
    group.finish();
}

fn bench_type_annotation_add(c: &mut Criterion) {
    let base_content = generate_base_document();
    let uri = "file:///test.graphql".parse::<Uri>().unwrap();

    let doc = DocumentState::new_from_thread_local(
        uri.clone(),
        &base_content,
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
        b.iter_with_setup(
            || doc.clone(),
            |mut d| {
                d.apply_change(&change, &mut update_parser, d.version + 1);
            },
        )
    });
    group.finish();
}

fn bench_large_document_edit(c: &mut Criterion) {
    let mut large_content = String::from("query Base { id }\n");
    for i in 0..500 {
        large_content.push_str(&format!(
            "fragment Fragment{} on User {{ id name email address {{ street city }} }}\n",
            i
        ));
    }
    large_content.push_str(
        r#"query GetUser {
  user(id: "123") {
    id
    name
    email
  }
}
"#,
    );

    let uri = "file:///large.graphql".parse::<Uri>().unwrap();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let doc = DocumentState::new(
        uri.clone(),
        &large_content,
        &mut parser,
        PositionEncodingKind::UTF8,
    );

    let mut update_parser = Parser::new();
    update_parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(0, 15), Position::new(0, 15))),
        range_length: None,
        text: " name".to_string(),
    };

    let mut group = c.benchmark_group("Document Editing - Large Document");
    group.sample_size(10);
    group.bench_function("Small edit in large document", |b| {
        b.iter_with_setup(
            || doc.clone(),
            |mut d| {
                d.apply_change(&change, &mut update_parser, d.version + 1);
            },
        )
    });
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_single_insert, bench_multiline_insert, bench_fragment_spread_add, bench_type_annotation_add, bench_large_document_edit
);
criterion_main!(benches);
