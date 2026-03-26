/// Performance tests for exception handling paths.
/// Compares throw/catch overhead against normal control flow.
///
/// Run with: cargo test --test perf_exceptions -- --nocapture --ignored
mod common;
use common::{run_php_silent, PreparedPhp};
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
    eprintln!("\n=== Closure vs Named Function (runtime-only) ===\n");

    // All hot loops wrapped in a function to avoid main-scope globals sync overhead.
    // Without this, every call from top-level syncs ALL CVs to eg.globals (clone + HashMap::insert).

    bench_rt("named function call (10k)", &format!(r#"<?php
function add($a, $b) {{
    return $a + $b;
}}
function bench_named() {{
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + add($i, 1);
    }}
    return $sum;
}}
bench_named();
"#));

    bench_rt("closure call (10k)", &format!(r#"<?php
function bench_closure() {{
    $add = function($a, $b) {{
        return $a + $b;
    }};
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $add($i, 1);
    }}
    return $sum;
}}
bench_closure();
"#));

    bench_rt("closure use int (10k)", &format!(r#"<?php
function bench_closure_int() {{
    $base = 100;
    $add = function($x) use ($base) {{
        return $base + $x;
    }};
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $add($i);
    }}
    return $sum;
}}
bench_closure_int();
"#));

    bench_rt("closure use string (10k)", &format!(r#"<?php
function bench_closure_str() {{
    $prefix = "hello";
    $f = function($x) use ($prefix) {{
        return $x;
    }};
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $f($i);
    }}
    return $sum;
}}
bench_closure_str();
"#));

    bench_rt("closure use 3 ints (10k)", &format!(r#"<?php
function bench_closure_3int() {{
    $a = 1;
    $b = 2;
    $c = 3;
    $f = function($x) use ($a, $b, $c) {{
        return $a + $b + $c + $x;
    }};
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $f($i);
    }}
    return $sum;
}}
bench_closure_3int();
"#));
}

#[test]
#[ignore]
fn perf_main_scope_tax() {
    eprintln!("\n=== Main-scope Tax: top-level vs wrapper (runtime-only) ===\n");

    // Top-level: calls from main scope — measures globals sync overhead.
    bench_rt("top-level: function (10k)", &format!(r#"<?php
function add($a, $b) {{
    return $a + $b;
}}
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + add($i, 1);
}}
"#));

    // Wrapper: same code but hot loop inside a function — no globals sync.
    bench_rt("wrapper: function (10k)", &format!(r#"<?php
function add($a, $b) {{
    return $a + $b;
}}
function bench_it() {{
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + add($i, 1);
    }}
    return $sum;
}}
bench_it();
"#));

    bench_rt("top-level: closure use int (10k)", &format!(r#"<?php
$base = 100;
$add = function($x) use ($base) {{
    return $base + $x;
}};
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $add($i);
}}
"#));

    bench_rt("wrapper: closure use int (10k)", &format!(r#"<?php
function bench_it() {{
    $base = 100;
    $add = function($x) use ($base) {{
        return $base + $x;
    }};
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $add($i);
    }}
    return $sum;
}}
bench_it();
"#));
}

#[test]
#[ignore]
fn perf_method_call() {
    eprintln!("\n=== Method Call vs Function Call (runtime-only) ===\n");

    // All hot loops wrapped in a function to avoid main-scope globals sync overhead.

    bench_rt("function call (10k)", &format!(r#"<?php
function compute($x) {{
    return $x + 1;
}}
function bench_func() {{
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + compute($i);
    }}
    return $sum;
}}
bench_func();
"#));

    bench_rt("method call (10k)", &format!(r#"<?php
class Math {{
    public function compute($x) {{
        return $x + 1;
    }}
}}
function bench_method() {{
    $m = new Math();
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $m->compute($i);
    }}
    return $sum;
}}
bench_method();
"#));

    bench_rt("method empty body (10k)", &format!(r#"<?php
class Noop {{
    public function run() {{
        return 0;
    }}
}}
function bench_noop() {{
    $n = new Noop();
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $n->run();
    }}
    return $sum;
}}
bench_noop();
"#));

    bench_rt("method reads $this prop (10k)", &format!(r#"<?php
class Counter {{
    public $val = 1;
    public function get() {{
        return $this->val;
    }}
}}
function bench_prop_read() {{
    $c = new Counter();
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $c->get();
    }}
    return $sum;
}}
bench_prop_read();
"#));

    bench_rt("static method call (10k)", &format!(r#"<?php
class SMath {{
    public static function compute($x) {{
        return $x + 1;
    }}
}}
function bench_static() {{
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + SMath::compute($i);
    }}
    return $sum;
}}
bench_static();
"#));

    bench_rt("property write (10k)", &format!(r#"<?php
class Box {{
    public $val = 0;
}}
function bench_prop_write() {{
    $b = new Box();
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $b->val = $i;
    }}
}}
bench_prop_write();
"#));

    bench_rt("property read+write (10k)", &format!(r#"<?php
class Acc {{
    public $sum = 0;
    public function add($x) {{
        $this->sum = $this->sum + $x;
    }}
}}
function bench_rw() {{
    $a = new Acc();
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $a->add($i);
    }}
}}
bench_rw();
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
