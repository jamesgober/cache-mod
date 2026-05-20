//! Criterion benchmarks for the five cache types.
//!
//! Each cache is exercised with a steady workload: pre-warmed to capacity,
//! then `get` and `insert` are measured in tight loops. Numbers are
//! intentionally indicative — a regression detector, not a marketing tool.
//!
//! Cargo skips this bench under `--no-default-features` via
//! `required-features = ["std"]` in `Cargo.toml`.

use std::hint::black_box;
use std::time::Duration;

use cache_mod::{Cache, LfuCache, LruCache, SizedCache, TinyLfuCache, TtlCache};
use criterion::{criterion_group, criterion_main, Criterion};

const CAPACITY: usize = 1024;

fn bench_lru(c: &mut Criterion) {
    let cache: LruCache<u32, u32> = LruCache::new(CAPACITY).expect("capacity > 0");
    for i in 0..CAPACITY as u32 {
        let _ = cache.insert(i, i);
    }
    let mut group = c.benchmark_group("LruCache");
    group.bench_function("get_hit", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.get(black_box(&(i % CAPACITY as u32)));
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.bench_function("insert_existing", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.insert(black_box(i % CAPACITY as u32), i);
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.finish();
}

fn bench_lfu(c: &mut Criterion) {
    let cache: LfuCache<u32, u32> = LfuCache::new(CAPACITY).expect("capacity > 0");
    for i in 0..CAPACITY as u32 {
        let _ = cache.insert(i, i);
    }
    let mut group = c.benchmark_group("LfuCache");
    group.bench_function("get_hit", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.get(black_box(&(i % CAPACITY as u32)));
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.bench_function("insert_existing", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.insert(black_box(i % CAPACITY as u32), i);
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.finish();
}

fn bench_ttl(c: &mut Criterion) {
    let cache: TtlCache<u32, u32> =
        TtlCache::new(CAPACITY, Duration::from_secs(3600)).expect("capacity > 0");
    for i in 0..CAPACITY as u32 {
        let _ = cache.insert(i, i);
    }
    let mut group = c.benchmark_group("TtlCache");
    group.bench_function("get_hit", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.get(black_box(&(i % CAPACITY as u32)));
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.bench_function("insert_existing", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.insert(black_box(i % CAPACITY as u32), i);
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.finish();
}

fn bench_tinylfu(c: &mut Criterion) {
    let cache: TinyLfuCache<u32, u32> = TinyLfuCache::new(CAPACITY).expect("capacity > 0");
    for i in 0..CAPACITY as u32 {
        // Repeat each key a few times to build sketch frequency so admission
        // doesn't reject everything in the warm-up.
        for _ in 0..4 {
            let _ = cache.insert(i, i);
        }
    }
    let mut group = c.benchmark_group("TinyLfuCache");
    group.bench_function("get_hit", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.get(black_box(&(i % CAPACITY as u32)));
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.bench_function("insert_existing", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.insert(black_box(i % CAPACITY as u32), i);
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.finish();
}

fn bench_sized(c: &mut Criterion) {
    fn weigh(_: &u32) -> usize {
        4
    }
    // max_weight = CAPACITY * 4 so the bound matches CAPACITY entries.
    let cache: SizedCache<u32, u32> = SizedCache::new(CAPACITY * 4, weigh).expect("max_weight > 0");
    for i in 0..CAPACITY as u32 {
        let _ = cache.insert(i, i);
    }
    let mut group = c.benchmark_group("SizedCache");
    group.bench_function("get_hit", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.get(black_box(&(i % CAPACITY as u32)));
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.bench_function("insert_existing", |b| {
        let mut i = 0u32;
        b.iter(|| {
            let v = cache.insert(black_box(i % CAPACITY as u32), i);
            i = i.wrapping_add(1);
            black_box(v)
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_lru,
    bench_lfu,
    bench_ttl,
    bench_tinylfu,
    bench_sized
);
criterion_main!(benches);
