//! Latency distribution test for the matching engine.
//!
//! Runs N iterations of a workload, records per-iteration wall-clock time
//! using `Instant::now()`, then prints p50/p99/p999 + throughput to stdout.
//!
//! Run with:
//!     cargo test -p matching-engine --test latency --release -- --nocapture
//!
//! This complements the criterion throughput benchmarks (which report
//! mean/median/std-dev) by giving the explicit percentiles called for in
//! CLAUDE.md Phase 1's exit criteria.

use std::time::Instant;

use common::{Order, OrderId, Price, Qty, Side, Timestamp};
use matching_engine::MatchingEngine;

/// Number of measured iterations per workload.
const ITERS: usize = 2_000;

/// Helper: build a fresh engine with `n_asks` sell levels (100..100+n)
/// at qty=10 each. Not measured.
fn build_book(n_asks: usize) -> MatchingEngine {
    let mut engine = MatchingEngine::new();
    for i in 0..n_asks {
        let id = (i + 1) as u64;
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
    engine
}

/// Run `workload` for warmup iterations then ITERS measured iterations,
/// sampling per-iter latency. Returns sorted ascending nanoseconds samples.
fn measure<F>(name: &str, mut workload: F)
where
    F: FnMut() -> (),
{
    // Warmup
    for _ in 0..1000 {
        workload();
    }
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        workload();
        samples.push(start.elapsed().as_nanos() as u64);
    }
    samples.sort_unstable();
    report(name, &samples);
}

fn report(name: &str, samples: &[u64]) {
    let n = samples.len();
    let sum: u64 = samples.iter().sum();
    let mean = sum / n as u64;
    let p50 = samples[n / 2];
    let p90 = samples[(n as f64 * 0.90) as usize];
    let p99 = samples[(n as f64 * 0.99) as usize];
    let p999 = samples[(n as f64 * 0.999) as usize];
    let max = samples[n - 1];
    let min = samples[0];
    let total_secs = (sum as f64) / 1e9;
    let throughput = n as f64 / total_secs;

    println!("\n=== Latency distribution: {} ===", name);
    println!("  iters:        {}", n);
    println!("  min  (ns):    {}", min);
    println!("  mean (ns):    {}", mean);
    println!("  p50  (ns):    {}", p50);
    println!("  p90  (ns):    {}", p90);
    println!("  p99  (ns):    {}", p99);
    println!("  p999 (ns):    {}", p999);
    println!("  max  (ns):    {}", max);
    println!("  throughput:   {:.0} ops/sec", throughput);
}

#[test]
fn latency_setup_engine_with_1000_asks() {
    let mut id_counter: u64 = 0;
    measure("setup_engine_with_1000_asks", || {
        let mut engine = MatchingEngine::new();
        for i in 0..1000 {
            id_counter += 1;
            let _ = engine.submit_limit(Order::limit(
                OrderId(id_counter),
                Side::Sell,
                Price(100 + i as u64),
                Qty(10),
                Timestamp(id_counter),
            ));
        }
        std::hint::black_box(engine);
    });
}

#[test]
fn latency_submit_limit_matched_against_1000_asks() {
    let mut id_counter: u64 = 10_000;
    let mut engine = build_book(1000);
    measure("submit_limit_matched_against_1000_asks", || {
        id_counter += 1;
        let order = Order::limit(
            OrderId(id_counter),
            Side::Buy,
            Price(50), // well below all asks
            Qty(10),
            Timestamp(id_counter),
        );
        let r = engine.submit_limit(order);
        let _ = std::hint::black_box(r);
        // Replenish the consumed maker so the book doesn't drain.
        let maker_id = id_counter + 1;
        id_counter += 1;
        let _ = engine.submit_limit(Order::limit(
            OrderId(maker_id),
            Side::Sell,
            Price(100),
            Qty(10),
            Timestamp(maker_id),
        ));
    });
}

#[test]
fn latency_submit_limit_rested() {
    let mut id_counter: u64 = 10_000;
    let mut engine = MatchingEngine::new();
    measure("submit_limit_rested", || {
        id_counter += 1;
        let order = Order::limit(
            OrderId(id_counter),
            Side::Buy,
            Price(10), // far from any potential ask
            Qty(10),
            Timestamp(id_counter),
        );
        let r = engine.submit_limit(order);
        let _ = std::hint::black_box(r);
        // Reset to a fresh engine periodically so the book doesn't grow unbounded.
        if id_counter % 500 == 0 {
            engine = MatchingEngine::new();
        }
    });
}

#[test]
fn latency_submit_market_against_1000_asks() {
    let mut id_counter: u64 = 10_000;
    let mut engine = build_book(1000);
    measure("submit_market_against_1000_asks", || {
        id_counter += 1;
        let order = Order::market(
            OrderId(id_counter),
            Side::Buy,
            Qty(10),
            Timestamp(id_counter),
        );
        let r = engine.submit_market(order);
        let _ = std::hint::black_box(r);
        // Replenish the consumed maker so the book doesn't drain.
        let maker_id = id_counter + 1;
        id_counter += 1;
        let _ = engine.submit_limit(Order::limit(
            OrderId(maker_id),
            Side::Sell,
            Price(100),
            Qty(10),
            Timestamp(maker_id),
        ));
    });
}

#[test]
fn latency_cancel_random_order() {
    let mut engine = build_book(1000);
    let mut id_counter: u64 = 100_000;
    measure("cancel_random_order", || {
        // Cancel one order and replace it so the engine stays in a steady state.
        id_counter += 1;
        let _ = engine.cancel(OrderId(500));
        let _ = engine.submit_limit(Order::limit(
            OrderId(id_counter),
            Side::Sell,
            Price(600), // a fresh price level
            Qty(10),
            Timestamp(id_counter),
        ));
        std::hint::black_box(&engine);
    });
}