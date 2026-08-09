// ── Baseline: tight loop with no exception path ──

#[test]
#[ignore] // only run explicitly
fn perf_baseline_loop() {
    eprintln!("\n=== Exception Performance Benchmarks ===\n");

    let baseline = bench(
        "baseline: for loop (10k iterations)",
        &format!(
            r#"<?php
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $i;
}}
"#
        ),
    );

    let try_no_throw = bench(
        "try/catch in loop, no throw",
        &format!(
            r#"<?php
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        $sum = $sum + $i;
    }} catch (Exception $e) {{
        $sum = 0;
    }}
}}
"#
        ),
    );

    let try_finally_no_throw = bench(
        "try/finally in loop, no throw",
        &format!(
            r#"<?php
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        $sum = $sum + $i;
    }} finally {{
    }}
}}
"#
        ),
    );

    eprintln!();
    eprintln!(
        "  try/catch overhead (no throw): {:.1}x baseline",
        try_no_throw / baseline
    );
    eprintln!(
        "  try/finally overhead (no throw): {:.1}x baseline",
        try_finally_no_throw / baseline
    );
}

#[test]
#[ignore]
fn perf_throw_catch_loop() {
    eprintln!("\n=== Throw/Catch in Loop ===\n");

    bench(
        "throw+catch per iteration (10k)",
        &format!(
            r#"<?php
$count = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    try {{
        throw new Exception("err");
    }} catch (Exception $e) {{
        $count = $count + 1;
    }}
}}
"#
        ),
    );

    bench(
        "throw across function call (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );
}

#[test]
#[ignore]
fn perf_finally_paths() {
    eprintln!("\n=== Finally Block Paths ===\n");

    bench(
        "try/finally with return (1k)",
        r#"<?php
function f() {
    try {
        return 1;
    } finally {
    }
}
for ($i = 0; $i < 1000; $i = $i + 1) {
    f();
}
"#,
    );

    bench(
        "try/catch/finally with throw (1k)",
        r#"<?php
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
"#,
    );

    bench(
        "nested try/finally propagation (1k)",
        r#"<?php
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
"#,
    );
}
