#[test]
#[ignore]
fn perf_closure_vs_function() {
    eprintln!("\n=== Closure vs Named Function (runtime-only) ===\n");

    // All hot loops wrapped in a function to avoid main-scope globals sync overhead.
    // Without this, every call from top-level syncs ALL CVs to eg.globals (clone + HashMap::insert).

    bench_rt(
        "named function call (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "closure call (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "closure use int (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "closure use string (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "closure use 3 ints (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );
}

#[test]
#[ignore]
fn perf_main_scope_tax() {
    eprintln!("\n=== Main-scope Tax: top-level vs wrapper (runtime-only) ===\n");

    // Top-level: calls from main scope — measures globals sync overhead.
    bench_rt(
        "top-level: function (10k)",
        &format!(
            r#"<?php
function add($a, $b) {{
    return $a + $b;
}}
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + add($i, 1);
}}
"#
        ),
    );

    // Wrapper: same code but hot loop inside a function — no globals sync.
    bench_rt(
        "wrapper: function (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "top-level: closure use int (10k)",
        &format!(
            r#"<?php
$base = 100;
$add = function($x) use ($base) {{
    return $base + $x;
}};
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + $add($i);
}}
"#
        ),
    );

    bench_rt(
        "wrapper: closure use int (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );
}

#[test]
#[ignore]
fn perf_array_access() {
    eprintln!("\n=== Array Access (runtime-only) ===\n");

    bench_rt(
        "int-key array read (10k)",
        &format!(
            r#"<?php
function bench_int_read() {{
    $arr = [10, 20, 30, 40, 50];
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $arr[2];
    }}
    return $sum;
}}
bench_int_read();
"#
        ),
    );

    bench_rt(
        "string-key array read (10k)",
        &format!(
            r#"<?php
function bench_str_read() {{
    $arr = ["name" => "John", "age" => 30, "city" => "Prague", "score" => 100];
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $sum = $sum + $arr["score"];
    }}
    return $sum;
}}
bench_str_read();
"#
        ),
    );

    bench_rt(
        "string-key array write (10k)",
        &format!(
            r#"<?php
function bench_str_write() {{
    $arr = ["x" => 0];
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $arr["x"] = $i;
    }}
    return $arr["x"];
}}
bench_str_write();
"#
        ),
    );
}

#[test]
#[ignore]
fn perf_method_call() {
    eprintln!("\n=== Method Call vs Function Call (runtime-only) ===\n");

    // All hot loops wrapped in a function to avoid main-scope globals sync overhead.

    bench_rt(
        "function call (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "method call (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "method empty body (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "method reads $this prop (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "static method call (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "property write (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );

    bench_rt(
        "property read+write (10k)",
        &format!(
            r#"<?php
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
"#
        ),
    );
}
