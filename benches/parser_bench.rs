use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::hint::black_box;

fn bench_parse_all(c: &mut Criterion) {
    let iters = 10_000;
    let (natural, shuffled) = hft_latency_lab::data::gen::generate_paired_streams(
        iters, iters / 2, iters / 4,
    );

    let mut group = c.benchmark_group("itch_parse");

    group.bench_function("naive_natural", |b| {
        b.iter(|| {
            black_box(hft_latency_lab::parser::naive::parse_all(black_box(&natural)));
        })
    });

    group.bench_function("optimized_natural", |b| {
        b.iter(|| {
            black_box(hft_latency_lab::parser::optimized::parse_all(black_box(&natural)));
        })
    });

    group.bench_function("optimized_shuffled", |b| {
        b.iter(|| {
            black_box(hft_latency_lab::parser::optimized::parse_all(black_box(&shuffled)));
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse_all);
criterion_main!(benches);
