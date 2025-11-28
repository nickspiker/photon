use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn crypto_benchmarks(_c: &mut Criterion) {
    // TODO: Add benchmarks for rolling chain encryption
}

criterion_group!(benches, crypto_benchmarks);
criterion_main!(benches);
