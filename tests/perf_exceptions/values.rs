#[test]
#[ignore]
fn perf_null_coalescing() {
    eprintln!("\n=== Null Coalescing vs Isset ===\n");

    bench(
        "isset check (10k)",
        &format!(
            r#"<?php
$val = 42;
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    if (isset($val)) {{
        $sum = $sum + $val;
    }}
}}
"#
        ),
    );

    bench(
        "?? operator (10k)",
        &format!(
            r#"<?php
$val = 42;
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + ($val ?? 0);
}}
"#
        ),
    );

    bench(
        "?? on null (10k)",
        &format!(
            r#"<?php
$val = null;
$sum = 0;
for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
    $sum = $sum + ($val ?? 1);
}}
"#
        ),
    );
}

#[test]
#[ignore]
fn perf_string_operations() {
    eprintln!("\n=== String Operations (runtime-only) ===\n");

    bench_rt(
        "string assign (10k)",
        &format!(
            r#"<?php
function bench_str_assign() {{
    $s = "hello world this is a test string";
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $copy = $s;
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_str_assign();
"#
        ),
    );

    bench_rt(
        "string pass+return (10k)",
        &format!(
            r#"<?php
function identity($s) {{
    return $s;
}}
function bench_str_pass() {{
    $s = "hello world this is a test string";
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $r = identity($s);
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_str_pass();
"#
        ),
    );

    bench_rt(
        "string concat (10k)",
        &format!(
            r#"<?php
function bench_str_concat() {{
    $a = "hello";
    $b = " world";
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $c = $a . $b;
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_str_concat();
"#
        ),
    );

    bench_rt(
        "string .= append (10k)",
        &format!(
            r#"<?php
function bench_str_append() {{
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $s = "base";
        $s .= "x";
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_str_append();
"#
        ),
    );

    bench_rt(
        "string echo (10k)",
        &format!(
            r#"<?php
function bench_str_echo() {{
    $s = "x";
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        echo $s;
    }}
}}
bench_str_echo();
"#
        ),
    );

    bench_rt(
        "string . int concat (10k)",
        &format!(
            r#"<?php
function bench_str_int_concat() {{
    $s = "val:";
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $c = $s . $i;
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_str_int_concat();
"#
        ),
    );

    bench_rt(
        "echo int (10k)",
        &format!(
            r#"<?php
function bench_echo_int() {{
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        echo $i;
    }}
}}
bench_echo_int();
"#
        ),
    );
}

#[test]
#[ignore]
fn perf_array_cow() {
    eprintln!("\n=== Array COW (runtime-only) ===\n");

    bench_rt(
        "array copy (10k)",
        &format!(
            r#"<?php
function bench_arr_copy() {{
    $arr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $copy = $arr;
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_arr_copy();
"#
        ),
    );

    bench_rt(
        "array pass+return (10k)",
        &format!(
            r#"<?php
function identity_arr($a) {{
    return $a;
}}
function bench_arr_pass() {{
    $arr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $r = identity_arr($arr);
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_arr_pass();
"#
        ),
    );

    bench_rt(
        "array copy+mutate (10k)",
        &format!(
            r#"<?php
function bench_arr_cow() {{
    $arr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $copy = $arr;
        $copy[] = 11;
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_arr_cow();
"#
        ),
    );

    bench_rt(
        "array string-key copy (10k)",
        &format!(
            r#"<?php
function bench_str_arr_copy() {{
    $arr = ['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4, 'e' => 5];
    $sum = 0;
    for ($i = 0; $i < {LOOP_COUNT}; $i = $i + 1) {{
        $copy = $arr;
        $sum = $sum + 1;
    }}
    return $sum;
}}
bench_str_arr_copy();
"#
        ),
    );
}
