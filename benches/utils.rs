use criterion::{Criterion, criterion_group, criterion_main};
use graphox::utils::merge_schema_texts;
use std::time::Duration;

fn generate_large_schema(start_idx: usize, count: usize) -> String {
    let mut schema = String::new();
    for i in 0..count {
        let idx = start_idx + i;
        schema.push_str(&format!(
            "\"\"\"Description for User{}\"\"\"\ntype User{} {{ id: ID! name: String }}\n",
            idx, idx
        ));
    }
    for i in 0..50 {
        schema.push_str(&format!("scalar Scalar{}\n", i));
    }
    schema
}

fn bench_merge_schema_texts(c: &mut Criterion) {
    let schema1 = generate_large_schema(0, 1000);
    let schema2 = generate_large_schema(500, 1000); // 500 overlapping types, all 50 scalars overlap
    let texts = vec![schema1, schema2];

    let mut group = c.benchmark_group("Utils - Schema Merging");
    group.sample_size(10);
    group.bench_function("merge_schema_texts (2x1000 types, 50% overlap)", |b| {
        b.iter(|| merge_schema_texts(&texts))
    });
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_millis(800)).measurement_time(Duration::from_millis(1000));
    targets = bench_merge_schema_texts
);
criterion_main!(benches);
