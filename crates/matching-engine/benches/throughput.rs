//! Throughput and latency benchmarks for the matching engine.
//!
//! Run with `cargo bench -p matching-engine` for full criterion output
//! (mean/median/std-dev + throughput). For p50/p99/p999 latency, run
//! `cargo test -p matching-engine --test latency -- --nocapture`.

use common::{Order, OrderId, Price, Qty, Side, Timestamp};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use matching_engine::MatchingEngine;

/// Build an engine with `n` ask levels (100..100+n) and `m` bid levels
/// (99..99-m, descending). Each level has 10 units of resting qty.
fn setup_book(asks: usize, bids: usize) -> MatchingEngine {
    let mut engine = MatchingEngine::new();
    let mut id = 0u64;
    for i in 0..asks {
        id += 1;
        engine
            .submit_limit(Order::limit(
                OrderId(id),
                Side::Sell,
                Price(100 + i as u64),
                Qty(10),
                Timestamp(id),
            ))
            .expect("setup ask accepted");
    }
    for i in 0..bids {
        id += 1;
        engine
            .submit_limit(Order::limit(
                OrderId(id),
                Side::Buy,
                Price(99 - i as u64),
                Qty(10),
                Timestamp(id),
            ))
            .expect("setup bid accepted");
    }
    engine
}

/// Submit a fully-matching limit buy at a price below all asks. Each
/// iteration consumes one ask level and adds a (slightly better) bid.
fn bench_submit_limit_matched(c: &mut Criterion) {
    let mut group = c.benchmark_group("submit_limit_matched");
    for n in [100usize, 1_000, 10_000].iter() {
        group.throughput(Throughput::Elements(*n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter_custom(|iters| {
                let mut total = std::time::Duration::ZERO;
                for _ in 0..iters {
                    let mut engine = setup_book(n, 0);
                    let mut id = (n as u64) + 1;
                    let start = std::time::Instant::now();
                    for _ in 0..n {
                        id += 1;
                        let order = Order::limit(
                            OrderId(id),
                            Side::Buy,
                            Price(50), // well below all asks
                            Qty(10),
                            Timestamp(id),
                        );
                        let _ = engine.submit_limit(order);
                    }
                    total += start.elapsed();
                }
                total
            });
        });
    }
    group.finish();
}

/// Submit a non-crossing limit buy (price well below best ask). Each
/// iteration appends to an ever-growing book.
fn bench_submit_limit_rested(c: &mut Criterion) {
    let mut group = c.benchmark_group("submit_limit_rested");
    for n in [100usize, 1_000, 10_000].iter() {
        group.throughput(Throughput::Elements(*n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter_custom(|iters| {
                let mut total = std::time::Duration::ZERO;
                for _ in 0..iters {
                    let mut engine = MatchingEngine::new();
                    let mut id = 1u64;
                    let start = std::time::Instant::now();
                    for _ in 0..n {
                        id += 1;
                        let order = Order::limit(
                            OrderId(id),
                            Side::Buy,
                            Price(10), // far from any potential ask
                            Qty(10),
                            Timestamp(id),
                        );
                        let _ = engine.submit_limit(order);
                    }
                    total += start.elapsed();
                }
                total
            });
        });
    }
    group.finish();
}

/// Submit a market buy that sweeps the existing asks.
fn bench_submit_market(c: &mut Criterion) {
    let mut group = c.benchmark_group("submit_market");
    for n in [100usize, 1_000, 10_000].iter() {
        group.throughput(Throughput::Elements(*n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter_custom(|iters| {
                let mut total = std::time::Duration::ZERO;
                for _ in 0..iters {
                    let mut engine = setup_book(n, 0);
                    let mut id = (n as u64) + 1;
                    let start = std::time::Instant::now();
                    for _ in 0..n {
                        id += 1;
                        let order = Order::market(
                            OrderId(id),
                            Side::Buy,
                            Qty(10),
                            Timestamp(id),
                        );
                        let _ = engine.submit_market(order);
                    }
                    total += start.elapsed();
                }
                total
            });
        });
    }
    group.finish();
}

/// Mixed workload: half matched, half rested. Simulates a busy session.
fn bench_mixed(c: &mut Criterion) {
    let mut id_counter: u64 = 0;
    c.bench_function("mixed_workload", |b| {
        b.iter_custom(|iters| {
            let mut engine = setup_book(500, 500);
            let start = std::time::Instant::now();
            for i in 0..iters {
                id_counter += 1;
                let (side, price) = if i % 2 == 0 {
                    // Matched: buy below asks.
                    (Side::Buy, Price(50))
                } else {
                    // Rested: buy below everything.
                    (Side::Buy, Price(10))
                };
                let order = Order::limit(
                    OrderId(id_counter),
                    side,
                    price,
                    Qty(10),
                    Timestamp(id_counter),
                );
                let _ = engine.submit_limit(order);
            }
            start.elapsed()
        });
    });
}

criterion_group!(
    benches,
    bench_submit_limit_matched,
    bench_submit_limit_rested,
    bench_submit_market,
    bench_mixed
);
criterion_main!(benches);