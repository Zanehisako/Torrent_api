use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn example_benchmark(c: &mut Criterion) {
    c.bench_function("example", |b| b.iter(|| black_box(2 + 2)));
}

criterion_group!(benches, example_benchmark);
criterion_main!(benches);
