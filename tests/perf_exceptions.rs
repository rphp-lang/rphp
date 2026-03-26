/// Performance tests for exception handling paths.
/// Compares throw/catch overhead against normal control flow.
///
/// Run with: cargo test --test perf_exceptions -- --nocapture --ignored
mod common;
use common::run_php_silent;
use std::time::Instant;

const WARMUP: u32 = 3;
const ITERATIONS: u32 = 10;

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
    eprintln!("  {:<45} median={:.3}ms  min={:.3}ms  max={:.3}ms", label, median, min, max);
    median
}

// ── Baseline: tight loop with no exception path ──

const LOOP_COUNT: &str = "10000";

#[test]
#[ignore] // only run explicitly
fn perf_baseline_loop() {
    eprintln!("\n=== Exception Performance Benchmarks ===\n");

    let baseline = bench("baseline: for loop (10k iterations)", &format!(r#"<?php
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $i;
}}
"#));

    let try_no_throw = bench("try/catch in loop, no throw", &format!(r#"<?php
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        $sum = $sum + $i;
    }} catch (Exception $e) {{
        $sum = 0;
    }}
}}
"#));

    let try_finally_no_throw = bench("try/finally in loop, no throw", &format!(r#"<?php
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        $sum = $sum + $i;
    }} finally {{
    }}
}}
"#));

    eprintln!();
    eprintln!("  try/catch overhead (no throw): {:.1}x baseline", try_no_throw / baseline);
    eprintln!("  try/finally overhead (no throw): {:.1}x baseline", try_finally_no_throw / baseline);
}

#[test]
#[ignore]
fn perf_throw_catch_loop() {
    eprintln!("\n=== Throw/Catch in Loop ===\n");

    bench("throw+catch per iteration (10k)", &format!(r#"<?php
$count = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        throw new Exception("err");
    }} catch (Exception $e) {{
        $count = $count + 1;
    }}
}}
"#));

    bench("throw across function call (10k)", &format!(r#"<?php
function thrower() {{
    throw new Exception("err");
}}
$count = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        thrower();
    }} catch (Exception $e) {{
        $count = $count + 1;
    }}
}}
"#));
}

#[test]
#[ignore]
fn perf_finally_paths() {
    eprintln!("\n=== Finally Block Paths ===\n");

    bench("try/finally with return (1k)", r#"<?php
function f() {
    try {
        return 1;
    } finally {
    }
}
for ($i = 0; $i < 1000; $i = $i + 1) {
    f();
}
"#);

    bench("try/catch/finally with throw (1k)", r#"<?php
function g() {
    try {
        throw new Exception("err");
    } catch (Exception $e) {
        return 1;
    } finally {
    }
}
for ($i = 0; $i < 1000; $i = $i + 1) {
    g();
}
"#);

    bench("nested try/finally propagation (1k)", r#"<?php
$count = 0;
for ($i = 0; $i < 1000; $i = $i + 1) {
    try {
        try {
            throw new Exception("inner");
        } finally {
            $count = $count + 1;
        }
    } catch (Exception $e) {
        $count = $count + 1;
    }
}
"#);
}

#[test]
#[ignore]
fn perf_closure_vs_function() {
    eprintln!("\n=== Closure vs Named Function ===\n");

    bench("named function call (10k)", &format!(r#"<?php
function add($a, $b) {{
    return $a + $b;
}}
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + add($i, 1);
}}
"#));

    bench("closure call (10k)", &format!(r#"<?php
$add = function($a, $b) {{
    return $a + $b;
}};
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $add($i, 1);
}}
"#));

    bench("closure with use var (10k)", &format!(r#"<?php
$base = 100;
$add = function($x) use ($base) {{
    return $base + $x;
}};
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $add($i);
}}
"#));
}

#[test]
#[ignore]
fn perf_method_call() {
    eprintln!("\n=== Method Call vs Function Call ===\n");

    bench("function call (10k)", &format!(r#"<?php
function compute($x) {{
    return $x + 1;
}}
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + compute($i);
}}
"#));

    bench("method call (10k)", &format!(r#"<?php
class Math {{
    public function compute($x) {{
        return $x + 1;
    }}
}}
$m = new Math();
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $m->compute($i);
}}
"#));

    bench("method empty body (10k)", &format!(r#"<?php
class Noop {{
    public function run() {{
        return 0;
    }}
}}
$n = new Noop();
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $n->run();
}}
"#));

    bench("method reads $this prop (10k)", &format!(r#"<?php
class Counter {{
    public $val = 1;
    public function get() {{
        return $this->val;
    }}
}}
$c = new Counter();
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $c->get();
}}
"#));

    bench("static method call (10k)", &format!(r#"<?php
class SMath {{
    public static function compute($x) {{
        return $x + 1;
    }}
}}
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + SMath::compute($i);
}}
"#));
}

#[test]
#[ignore]
fn perf_null_coalescing() {
    eprintln!("\n=== Null Coalescing vs Isset ===\n");

    bench("isset check (10k)", &format!(r#"<?php
$val = 42;
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    if (isset($val)) {{
        $sum = $sum + $val;
    }}
}}
"#));

    bench("?? operator (10k)", &format!(r#"<?php
$val = 42;
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + ($val ?? 0);
}}
"#));

    bench("?? on null (10k)", &format!(r#"<?php
$val = null;
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + ($val ?? 1);
}}
"#));
}
