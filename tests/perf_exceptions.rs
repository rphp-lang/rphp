/// Performance tests for exception handling paths.
/// Compares throw/catch overhead against normal control flow.
///
/// Run with: cargo test --test perf_exceptions -- --nocapture --ignored
mod common;
use common::{PreparedPhp, run_php_silent};
use std::time::Instant;

const WARMUP: u32 = 3;
const ITERATIONS: u32 = 10;
const LOOP_COUNT: &str = "10000";

fn bench(label: &str, source: &str) -> f64 {
    // Warmup
    for _ in 0..WARMUP {
        run_php_silent(source);
    }
    // Measure
    let mut times = Vec::with_capacity(ITERATIONS as usize);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        run_php_silent(source);
        times.push(start.elapsed().as_secs_f64() * 1000.0); // ms
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let min = times[0];
    let max = times[times.len() - 1];
    eprintln!(
        "  {:<45} median={:.3}ms  min={:.3}ms  max={:.3}ms",
        label, median, min, max
    );
    median
}

/// Runtime-only benchmark: compile once, execute many.
/// Eliminates lex/parse/compile noise. Inline caches stay warm (steady-state).
fn bench_rt(label: &str, source: &str) -> f64 {
    let mut prepared = PreparedPhp::new(source);
    // Warmup (also warms inline caches)
    for _ in 0..WARMUP {
        prepared.execute_silent();
    }
    // Measure
    let mut times = Vec::with_capacity(ITERATIONS as usize);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        prepared.execute_silent();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let min = times[0];
    let max = times[times.len() - 1];
    eprintln!(
        "  {:<45} median={:.3}ms  min={:.3}ms  max={:.3}ms",
        label, median, min, max
    );
    median
}

include!("perf_exceptions/exceptions.rs");
include!("perf_exceptions/calls.rs");
include!("perf_exceptions/values.rs");
