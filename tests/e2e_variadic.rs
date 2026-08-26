/// E2E tests: variadic parameters (...$args)
mod common;
use common::run_php;

// ============================================================================
// Basic variadic
// ============================================================================

#[test]
fn test_variadic_basic() {
    assert_eq!(
        run_php(
            r#"<?php
function sum(...$nums) {
    $total = 0;
    foreach ($nums as $n) {
        $total += $n;
    }
    return $total;
}
echo sum(1, 2, 3);
"#
        ),
        "6"
    );
}

#[test]
fn test_variadic_no_args() {
    assert_eq!(
        run_php(
            r#"<?php
function collect(...$items) {
    echo count($items);
}
collect();
"#
        ),
        "0"
    );
}

#[test]
fn test_variadic_single_arg() {
    assert_eq!(
        run_php(
            r#"<?php
function first(...$args) {
    echo $args[0];
}
first(42);
"#
        ),
        "42"
    );
}

#[test]
fn test_variadic_with_fixed_params() {
    assert_eq!(
        run_php(
            r#"<?php
function greet($greeting, ...$names) {
    foreach ($names as $name) {
        echo $greeting . " " . $name . "\n";
    }
}
greet("Hello", "Alice", "Bob");
"#
        ),
        "Hello Alice\nHello Bob\n"
    );
}

#[test]
fn test_variadic_with_fixed_and_default() {
    assert_eq!(
        run_php(
            r#"<?php
function my_log($level = "INFO", ...$messages) {
    foreach ($messages as $msg) {
        echo "[" . $level . "] " . $msg . "\n";
    }
}
my_log("WARN", "disk full", "retry");
"#
        ),
        "[WARN] disk full\n[WARN] retry\n"
    );
}

#[test]
fn test_variadic_count() {
    assert_eq!(
        run_php(
            r#"<?php
function nargs(...$args) {
    return count($args);
}
echo nargs();
echo " ";
echo nargs(1);
echo " ";
echo nargs(1, 2, 3, 4, 5);
"#
        ),
        "0 1 5"
    );
}

#[test]
fn test_variadic_mixed_types() {
    assert_eq!(
        run_php(
            r#"<?php
function dump(...$args) {
    foreach ($args as $a) {
        echo gettype($a) . " ";
    }
}
dump(1, "hello", true, null, 3.14);
"#
        ),
        "integer string boolean NULL double "
    );
}

#[test]
fn test_variadic_pass_to_another_function() {
    assert_eq!(
        run_php(
            r#"<?php
function add($a, $b) {
    return $a + $b;
}
function apply_add(...$args) {
    return add($args[0], $args[1]);
}
echo apply_add(3, 7);
"#
        ),
        "10"
    );
}

#[test]
fn test_variadic_in_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Logger {
    public function log($prefix, ...$messages) {
        foreach ($messages as $msg) {
            echo $prefix . ": " . $msg . "\n";
        }
    }
}
$l = new Logger();
$l->log("APP", "started", "ready");
"#
        ),
        "APP: started\nAPP: ready\n"
    );
}

#[test]
fn test_variadic_in_closure() {
    assert_eq!(
        run_php(
            r#"<?php
$sum = function(...$nums) {
    $t = 0;
    foreach ($nums as $n) { $t += $n; }
    return $t;
};
echo $sum(10, 20, 30);
"#
        ),
        "60"
    );
}

#[test]
fn test_variadic_in_arrow_function() {
    assert_eq!(
        run_php(
            r#"<?php
$count = fn(...$args) => count($args);
echo $count(1, 2, 3);
"#
        ),
        "3"
    );
}

#[test]
fn test_variadic_is_array() {
    assert_eq!(
        run_php(
            r#"<?php
function check(...$args) {
    echo is_array($args) ? "yes" : "no";
}
check(1, 2);
"#
        ),
        "yes"
    );
}

#[test]
fn by_reference_variadic_tail_preserves_each_positional_named_and_unpacked_alias() {
    assert_eq!(
        run_php(
            r#"<?php
function rewrite($base, &...$slots) {
    $next = $base;
    foreach ($slots as &$slot) { $slot = $next++; }
}
class VariadicWriter {
    public function rewrite($base, &...$slots) {
        $next = $base;
        foreach ($slots as &$slot) { $slot = $next++; }
    }
}
$closure = function ($base, &...$slots) {
    $next = $base;
    foreach ($slots as &$slot) { $slot = $next++; }
};

$a = 1; $b = 2; $c = 3;
rewrite(10, $a, $b, $c);
$d = 4; $e = 5;
(new VariadicWriter())->rewrite(20, $d, $e);
$f = 6; $g = 7;
$closure(30, $f, $g);
$h = 8; $i = 9;
rewrite(40, left: $h, right: $i);
$packed = [10, 11];
rewrite(50, ...$packed);
echo "$a,$b,$c|$d,$e|$f,$g|$h,$i|", implode(',', $packed);
"#,
        ),
        "10,11,12|20,21|30,31|40,41|50,51"
    );
}

#[test]
fn by_reference_variadic_tail_remains_reference_aware_beyond_the_mask_width() {
    let declarations = (0..66)
        .map(|index| format!("$value{index} = {index};"))
        .collect::<Vec<_>>()
        .join("\n");
    let arguments = (0..66)
        .map(|index| format!("$value{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "<?php\n{declarations}\nrewrite({arguments});\necho \"$value0:$value63:$value64:$value65\";\nfunction rewrite(&...$slots) {{ foreach ($slots as $index => &$slot) {{ $slot = 100 + $index; }} }}\n"
    );

    assert_eq!(run_php(&source), "100:163:164:165");
}

#[test]
fn by_reference_variadic_errors_omit_the_bucket_name_across_call_forms() {
    assert_eq!(
        run_php(
            r#"<?php
function reject($fixed, &...$values) {}
class VariadicRejector { public static function reject($fixed, &...$values) {} }
$closure = function ($fixed, &...$values) {};
$slot = 0;

try { reject('direct', $slot, 1); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
try { VariadicRejector::reject('method', $slot, 1); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
try { $closure('closure', $slot, 1); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
try { reject(fixed: 'named', left: $slot, right: 1); }
catch (Error $error) { echo $error->getMessage(), "\n"; }

set_error_handler(function ($_level, $message) { echo "warning:$message\n"; });
call_user_func('reject', 'callback', 1, 2);
"#,
        ),
        concat!(
            "reject(): Argument #3 could not be passed by reference\n",
            "VariadicRejector::reject(): Argument #3 could not be passed by reference\n",
            "{closure}(): Argument #3 could not be passed by reference\n",
            "reject(): Argument #2 could not be passed by reference\n",
            "warning:reject(): Argument #2 must be passed by reference, value given\n",
            "warning:reject(): Argument #3 must be passed by reference, value given\n",
        )
    );
}
