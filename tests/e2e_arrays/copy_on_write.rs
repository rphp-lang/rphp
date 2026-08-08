// ── COW array aliasing regression tests ───────────────────────────

#[test]
fn test_e2e_array_cow_assign_then_push() {
    // $b = $a shares Rc. $b[] = must COW-detach, not mutate $a.
    assert_eq!(
        run_php(
            "<?php
$a = [1, 2, 3];
$b = $a;
$b[] = 4;
echo count($a) . '|' . count($b);
"
        ),
        "3|4"
    );
}

#[test]
fn test_e2e_array_cow_function_arg() {
    // Function arg is a clone (Rc bump). Push inside must not affect caller.
    assert_eq!(
        run_php(
            "<?php
function add_elem($arr) { $arr[] = 99; return count($arr); }
$x = [10, 20];
$y = add_elem($x);
echo count($x) . '|' . $y;
"
        ),
        "2|3"
    );
}

#[test]
fn test_e2e_array_cow_multiple_clones() {
    // Multiple clones from same source — each mutation independent.
    assert_eq!(
        run_php(
            "<?php
$s = [1];
$c1 = $s;
$c2 = $s;
$c1[] = 2;
$c2[] = 3;
echo count($s) . '|' . count($c1) . '|' . count($c2);
"
        ),
        "1|2|2"
    );
}

#[test]
fn test_e2e_array_cow_sole_owner_inplace() {
    // Sole owner push should mutate in place (no COW detach).
    assert_eq!(
        run_php(
            "<?php
$z = [1, 2];
$z[] = 3;
echo count($z);
"
        ),
        "3"
    );
}

#[test]
fn test_e2e_array_cow_string_key_mutation() {
    // String-keyed array — copy + add new key must not affect original.
    assert_eq!(
        run_php(
            "<?php
$m = ['a' => 1, 'b' => 2];
$n = $m;
$n['c'] = 3;
echo count($m) . '|' . count($n) . '|' . $n['c'];
"
        ),
        "2|3|3"
    );
}

#[test]
fn test_e2e_array_cow_overwrite_existing_key() {
    // Overwrite existing key on a shared copy — original unaffected.
    assert_eq!(
        run_php(
            "<?php
$a = ['x' => 10, 'y' => 20];
$b = $a;
$b['x'] = 999;
echo $a['x'] . '|' . $b['x'];
"
        ),
        "10|999"
    );
}

#[test]
fn test_e2e_array_cow_nested_function() {
    // Array passed through two function calls, mutated at end.
    assert_eq!(
        run_php(
            "<?php
function inner($arr) { $arr[] = 3; return $arr; }
function outer($arr) { return inner($arr); }
$x = [1, 2];
$y = outer($x);
echo count($x) . '|' . count($y);
"
        ),
        "2|3"
    );
}

#[test]
fn test_e2e_array_cow_foreach_isolation() {
    // Foreach iterates over a COW copy — mutation of original during loop.
    assert_eq!(
        run_php(
            "<?php
$arr = [1, 2, 3];
$sum = 0;
foreach ($arr as $v) {
    $sum = $sum + $v;
}
$arr[] = 4;
echo $sum . '|' . count($arr);
"
        ),
        "6|4"
    );
}

#[test]
fn test_e2e_array_cow_value_independence() {
    // Values inside array are independent after copy (string values).
    assert_eq!(
        run_php(
            "<?php
$a = ['hello', 'world'];
$b = $a;
$b[0] = 'changed';
echo $a[0] . '|' . $b[0];
"
        ),
        "hello|changed"
    );
}

#[test]
fn test_e2e_array_cow_closure_capture_mutate() {
    // Array captured by closure, mutated inside — outer unchanged.
    assert_eq!(
        run_php(
            "<?php
$arr = [1, 2, 3];
$fn = function() use ($arr) {
    $arr[] = 4;
    return count($arr);
};
echo count($arr) . '|' . $fn();
"
        ),
        "3|4"
    );
}

#[test]
fn test_e2e_array_cow_string_in_array_mutate() {
    // String stored in array, clone array, mutate string in clone — original string intact.
    assert_eq!(
        run_php(
            "<?php
$a = ['s' => 'hello'];
$b = $a;
$b['s'] = $b['s'] . ' world';
echo $a['s'] . '|' . $b['s'];
"
        ),
        "hello|hello world"
    );
}

#[test]
fn test_e2e_array_cow_large_array() {
    // 100-element array copy + mutate — COW detach scales correctly.
    assert_eq!(
        run_php(
            "<?php
$a = [];
for ($i = 0; $i < 100; $i = $i + 1) { $a[] = $i; }
$b = $a;
$b[50] = 999;
echo $a[50] . '|' . $b[50] . '|' . count($a) . '|' . count($b);
"
        ),
        "50|999|100|100"
    );
}

#[test]
fn test_e2e_array_cow_repeated_clone_mutate() {
    // Clone+mutate in a loop — each iteration gets a fresh COW detach.
    assert_eq!(
        run_php(
            "<?php
$base = [1, 2, 3];
$results = '';
for ($i = 0; $i < 3; $i = $i + 1) {
    $copy = $base;
    $copy[] = $i;
    $results .= count($copy) . ',';
}
echo count($base) . '|' . $results;
"
        ),
        "3|4,4,4,"
    );
}

#[test]
fn test_e2e_string_cow_in_closure_and_array() {
    // String shared via array AND closure capture — both paths independent.
    assert_eq!(
        run_php(
            "<?php
$s = 'shared';
$arr = ['v' => $s];
$fn = function() use ($s) { return $s . '!'; };
$s .= '_mutated';
echo $arr['v'] . '|' . $fn() . '|' . $s;
"
        ),
        "shared|shared!|shared_mutated"
    );
}

#[test]
fn test_e2e_string_cow_multiple_append() {
    // Multiple .= on same string — all sole-owner after initial detach.
    assert_eq!(
        run_php(
            "<?php
$s = 'a';
$s .= 'b';
$s .= 'c';
$s .= 'd';
$s .= 'e';
echo $s;
"
        ),
        "abcde"
    );
}

#[test]
fn test_e2e_array_cow_associative_large() {
    // Large associative array copy + string key mutation.
    assert_eq!(
        run_php(
            "<?php
$a = [];
for ($i = 0; $i < 50; $i = $i + 1) {
    $a['k' . $i] = 'v' . $i;
}
$b = $a;
$b['k25'] = 'changed';
echo $a['k25'] . '|' . $b['k25'] . '|' . count($b);
"
        ),
        "v25|changed|50"
    );
}
