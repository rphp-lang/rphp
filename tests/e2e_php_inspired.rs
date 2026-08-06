/// E2E tests inspired by php-src/tests/lang/*.phpt
/// Not 1:1 copies — adapted to our current feature set.
mod common;
use common::run_php;

// === Inspired by lang/017: function return value used in if condition ===

#[test]
fn test_php_func_return_as_if_condition() {
    // Function returns a value that gets compared directly in if()
    assert_eq!(
        run_php(
            "<?php function check($a) { if ($a < 3) { return 3; } return 0; } $a = 1; if ($a < check($a)) { echo $a; }"
        ),
        "1"
    );
}

#[test]
fn test_php_func_return_controls_loop() {
    // Function return value used to control a while loop
    assert_eq!(
        run_php(
            "<?php function limit() { return 5; } $i = 0; while ($i < limit()) { echo $i; $i++; }"
        ),
        "01234"
    );
}

// === Inspired by lang/022: nested switch inside function ===

#[test]
fn test_php_nested_switch_in_function() {
    // Function with nested switch, called from a loop
    assert_eq!(
        run_php(
            "<?php function classify($n) { switch ($n) { case 0: return 'z'; case 1: return 'o'; default: return 'd'; } } for ($i = 0; $i < 4; $i++) { echo classify($i); }"
        ),
        "zodd"
    );
}

#[test]
fn test_php_switch_with_nested_switch_in_function() {
    // Outer switch dispatches, inner switch refines
    assert_eq!(
        run_php(
            "<?php function test($i, $j) { switch ($i) { case 0: switch ($j) { case 0: echo 'a'; break; case 1: echo 'b'; break; default: echo 'c'; break; } break; default: echo 'x'; break; } } test(0, 0); test(0, 1); test(0, 5); test(1, 0);"
        ),
        "abcx"
    );
}

// === Inspired by lang/025: recursive function with while loop ===

#[test]
fn test_php_recursive_with_while() {
    // Recursive function that uses while internally
    assert_eq!(
        run_php(
            "<?php function countdown($n) { if ($n <= 0) { return 0; } echo $n; $j = $n - 1; while ($j > $n - 2) { countdown($j); $j = $j - 1; } return 0; } countdown(3);"
        ),
        "321"
    );
}

#[test]
fn test_php_recursive_sum() {
    // Classic recursive sum
    assert_eq!(
        run_php(
            "<?php function rsum($n) { if ($n <= 0) { return 0; } return $n + rsum($n - 1); } echo rsum(10);"
        ),
        "55"
    );
}

// === Inspired by lang/012: early return in function called from loop ===

#[test]
fn test_php_early_return_in_loop_call() {
    // Function with early return called repeatedly from a for loop
    assert_eq!(
        run_php(
            "<?php function maybe($n) { if ($n % 2 == 0) { return $n; } return 0; } $sum = 0; for ($i = 0; $i < 6; $i++) { $sum += maybe($i); } echo $sum;"
        ),
        "6"
    );
}

// === Inspired by lang/026: string escape sequences ===

#[test]
fn test_php_double_quote_escapes() {
    // Double-quoted string escape sequences
    assert_eq!(
        run_php(r#"<?php echo "tab:\there\nnewline";"#),
        "tab:\there\nnewline"
    );
}

#[test]
fn test_php_single_quote_escapes() {
    // Single-quoted: only \\ and \' are special, rest kept literally
    assert_eq!(
        run_php(r"<?php echo 'hello\nworld\\end';"),
        "hello\\nworld\\end"
    );
}

#[test]
fn test_php_escaped_quotes_in_strings() {
    // Escaped quote inside the same quote type
    assert_eq!(
        run_php(r#"<?php echo "she said \"hi\"";"#),
        "she said \"hi\""
    );
}

#[test]
fn test_php_single_escaped_apostrophe() {
    assert_eq!(run_php(r"<?php echo 'it\'s fine';"), "it's fine");
}

// === Inspired by lang/003/020/021: switch patterns ===

#[test]
fn test_php_switch_in_for_all_branches() {
    // Switch inside for loop — exercises every branch including default
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i <= 3; $i++) { switch ($i) { case 0: echo 'A'; break; case 1: echo 'B'; break; case 2: echo 'C'; break; default: echo 'D'; break; } }"
        ),
        "ABCD"
    );
}

#[test]
fn test_php_switch_fall_through_chain() {
    // Fall-through from case 1 through case 2 to case 3
    assert_eq!(
        run_php(
            "<?php $x = 1; switch ($x) { case 1: echo '1'; case 2: echo '2'; case 3: echo '3'; break; case 4: echo '4'; }"
        ),
        "123"
    );
}

#[test]
fn test_php_switch_string_comparison() {
    // String values in switch cases
    assert_eq!(
        run_php(
            "<?php $s = 'banana'; switch ($s) { case 'apple': echo 'A'; break; case 'banana': echo 'B'; break; case 'cherry': echo 'C'; break; default: echo 'X'; }"
        ),
        "B"
    );
}

// === Inspired by lang/002: while loop with multiple operations ===

#[test]
fn test_php_while_accumulate_string() {
    // Build a string in a while loop
    assert_eq!(
        run_php("<?php $s = ''; $i = 1; while ($i < 6) { $s .= $i; $i++; } echo $s;"),
        "12345"
    );
}

#[test]
fn test_php_nested_while_multiplication_table() {
    // Nested while producing a simple product table
    assert_eq!(
        run_php(
            "<?php $i = 1; while ($i <= 3) { $j = 1; while ($j <= 3) { echo $i * $j; $j++; } $i++; }"
        ),
        "123246369"
    );
}

// === Inspired by lang/006: nested if/elseif with multiple variables ===

#[test]
fn test_php_nested_if_two_vars() {
    assert_eq!(
        run_php(
            "<?php $a = 1; $b = 2; if ($a == 1) { if ($b == 1) { echo 'aa'; } elseif ($b == 2) { echo 'ab'; } else { echo 'ax'; } } else { echo 'no'; }"
        ),
        "ab"
    );
}

#[test]
fn test_php_elseif_chain_exhaustive() {
    // Exercise every branch in a long elseif chain
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 5; $i++) { if ($i == 0) { echo 'A'; } elseif ($i == 1) { echo 'B'; } elseif ($i == 2) { echo 'C'; } elseif ($i == 3) { echo 'D'; } else { echo 'E'; } }"
        ),
        "ABCDE"
    );
}

// === Combo tests: mixing constructs ===

#[test]
fn test_php_function_with_loop_and_switch() {
    // Function that uses a for loop with switch inside
    assert_eq!(
        run_php(
            "<?php function fizz($n) { $r = ''; for ($i = 1; $i <= $n; $i++) { switch ($i % 3) { case 0: $r .= 'F'; break; default: $r .= $i; break; } } return $r; } echo fizz(6);"
        ),
        "12F45F"
    );
}

#[test]
fn test_php_recursive_fibonacci() {
    assert_eq!(
        run_php(
            "<?php function fib($n) { if ($n <= 1) { return $n; } return fib($n - 1) + fib($n - 2); } echo fib(10);"
        ),
        "55"
    );
}

#[test]
fn test_php_loop_with_ternary() {
    assert_eq!(
        run_php(
            "<?php $r = ''; for ($i = 0; $i < 6; $i++) { $r .= ($i % 2 == 0) ? 'E' : 'O'; } echo $r;"
        ),
        "EOEOEO"
    );
}

// === Inspired by PHP tests: arrays + loops + functions combos ===

#[test]
fn test_php_array_reverse_manual() {
    // Manually reverse an array using index math
    assert_eq!(
        run_php(
            "<?php $a = [10, 20, 30, 40, 50]; $r = ''; for ($i = 4; $i >= 0; $i--) { $r .= $a[$i]; } echo $r;"
        ),
        "5040302010"
    );
}

#[test]
fn test_php_array_swap_elements() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3]; $tmp = $a[0]; $a[0] = $a[2]; $a[2] = $tmp; echo $a[0]; echo $a[1]; echo $a[2];"
        ),
        "321"
    );
}

#[test]
fn test_php_array_as_lookup_table() {
    // Use associative array as a translation table
    assert_eq!(
        run_php(
            "<?php $t = ['a' => '1', 'b' => '2', 'c' => '3']; echo $t['b']; echo $t['a']; echo $t['c'];"
        ),
        "213"
    );
}

#[test]
fn test_php_function_modifies_array_copy() {
    // Arrays are passed by value in PHP — function gets a copy
    assert_eq!(
        run_php(
            "<?php function add_elem($arr) { $arr[] = 99; return $arr[0]; } $a = [42]; echo add_elem($a); echo $a[0];"
        ),
        "4242"
    );
}

#[test]
fn test_php_nested_array_build_and_read() {
    assert_eq!(
        run_php(
            "<?php $matrix = []; for ($i = 0; $i < 3; $i++) { $row = []; for ($j = 0; $j < 3; $j++) { $row[] = $i * 3 + $j; } $matrix[] = $row; } echo $matrix[0][0]; echo $matrix[1][1]; echo $matrix[2][2];"
        ),
        "048"
    );
}

#[test]
fn test_php_break2_from_switch_in_loop_with_array() {
    // break 2 from switch inside loop — exercise arrays + break N
    assert_eq!(
        run_php(
            "<?php $cmds = ['go', 'go', 'stop', 'go']; for ($i = 0; $i < 4; $i++) { switch ($cmds[$i]) { case 'stop': break 2; default: echo $i; } } echo 'end';"
        ),
        "01end"
    );
}

#[test]
fn test_php_bubble_sort() {
    // Simple bubble sort
    assert_eq!(
        run_php(
            "<?php $a = [5, 3, 1, 4, 2]; for ($i = 0; $i < 5; $i++) { for ($j = 0; $j < 4; $j++) { if ($a[$j] > $a[$j + 1]) { $tmp = $a[$j]; $a[$j] = $a[$j + 1]; $a[$j + 1] = $tmp; } } } for ($i = 0; $i < 5; $i++) { echo $a[$i]; }"
        ),
        "12345"
    );
}
