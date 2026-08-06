/// E2E tests: isset, empty, unset, type casting, type checks.
mod common;
use common::run_php;

// ========== isset() ==========

#[test]
fn test_isset_defined_var() {
    assert_eq!(
        run_php("<?php $x = 42; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_undefined_var() {
    assert_eq!(run_php("<?php echo isset($x) ? 'yes' : 'no';"), "no");
}

#[test]
fn test_isset_null_var() {
    assert_eq!(
        run_php("<?php $x = null; echo isset($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_isset_zero_is_set() {
    assert_eq!(
        run_php("<?php $x = 0; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_empty_string_is_set() {
    assert_eq!(
        run_php("<?php $x = ''; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_false_is_set() {
    assert_eq!(
        run_php("<?php $x = false; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_multi_arg() {
    assert_eq!(
        run_php("<?php $a = 1; $b = 2; echo isset($a, $b) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_multi_arg_one_null() {
    assert_eq!(
        run_php("<?php $a = 1; $b = null; echo isset($a, $b) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_isset_rejects_expression() {
    let result =
        std::panic::catch_unwind(|| run_php("<?php $x = 1; echo isset($x + 1) ? 'yes' : 'no';"));
    assert!(result.is_err());
}

// ========== empty() ==========

#[test]
fn test_empty_undefined() {
    assert_eq!(run_php("<?php echo empty($x) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_empty_null() {
    assert_eq!(
        run_php("<?php $x = null; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_false() {
    assert_eq!(
        run_php("<?php $x = false; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_zero() {
    assert_eq!(
        run_php("<?php $x = 0; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_empty_string() {
    assert_eq!(
        run_php("<?php $x = ''; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_nonempty() {
    assert_eq!(
        run_php("<?php $x = 'hello'; echo empty($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_empty_array_empty() {
    assert_eq!(
        run_php("<?php $x = []; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_array_nonempty() {
    assert_eq!(
        run_php("<?php $x = [1]; echo empty($x) ? 'yes' : 'no';"),
        "no"
    );
}

// ========== unset() ==========

#[test]
fn test_unset_basic() {
    assert_eq!(
        run_php("<?php $x = 42; unset($x); echo isset($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_unset_multiple() {
    assert_eq!(
        run_php("<?php $a = 1; $b = 2; unset($a, $b); echo isset($a) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_unset_then_reassign() {
    assert_eq!(run_php("<?php $x = 1; unset($x); $x = 2; echo $x;"), "2");
}

#[test]
fn test_unset_array_element() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; unset($a[1]); echo count($a);"),
        "2"
    );
}

#[test]
fn test_unset_array_element_isset() {
    assert_eq!(
        run_php(
            "<?php $a = ['x' => 1, 'y' => 2]; unset($a['x']); echo isset($a['x']) ? 'yes' : 'no';"
        ),
        "no"
    );
}

#[test]
fn test_unset_array_preserves_other() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; unset($a[0]); echo $a[1] . $a[2];"),
        "23"
    );
}

// ========== Type casting ==========

#[test]
fn test_cast_int_from_float() {
    assert_eq!(run_php("<?php echo (int)3.7;"), "3");
}

#[test]
fn test_cast_int_from_string() {
    assert_eq!(run_php("<?php echo (int)'42abc';"), "42");
}

#[test]
fn test_cast_int_from_bool_true() {
    assert_eq!(run_php("<?php echo (int)true;"), "1");
}

#[test]
fn test_cast_int_from_bool_false() {
    assert_eq!(run_php("<?php echo (int)false;"), "0");
}

#[test]
fn test_cast_int_from_null() {
    assert_eq!(run_php("<?php echo (int)null;"), "0");
}

#[test]
fn test_cast_float_from_int() {
    assert_eq!(run_php("<?php $x = (float)42; echo $x + 0.5;"), "42.5");
}

#[test]
fn test_cast_float_from_string() {
    assert_eq!(run_php("<?php echo (float)'3.14';"), "3.14");
}

#[test]
fn test_cast_string_from_int() {
    assert_eq!(run_php("<?php $s = (string)42; echo strlen($s);"), "2");
}

#[test]
fn test_cast_string_from_float() {
    assert_eq!(run_php("<?php $s = (string)3.14; echo $s;"), "3.14");
}

#[test]
fn test_cast_string_from_bool() {
    assert_eq!(run_php("<?php echo (string)true;"), "1");
}

#[test]
fn test_cast_bool_truthy() {
    assert_eq!(run_php("<?php echo (bool)42 ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_cast_bool_falsy() {
    assert_eq!(run_php("<?php echo (bool)0 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_cast_array_from_scalar() {
    assert_eq!(
        run_php("<?php $a = (array)42; echo count($a) . ':' . $a[0];"),
        "1:42"
    );
}

#[test]
fn test_cast_array_from_null() {
    assert_eq!(run_php("<?php $a = (array)null; echo count($a);"), "0");
}

#[test]
fn test_cast_array_from_array() {
    assert_eq!(
        run_php("<?php $a = [1,2]; $b = (array)$a; echo count($b);"),
        "2"
    );
}

#[test]
fn test_cast_integer_keyword() {
    assert_eq!(run_php("<?php echo (integer)3.7;"), "3");
}

#[test]
fn test_cast_double_keyword() {
    assert_eq!(run_php("<?php $x = (double)42; echo $x + 0.5;"), "42.5");
}

#[test]
fn test_cast_boolean_keyword() {
    assert_eq!(run_php("<?php echo (boolean)1 ? 'yes' : 'no';"), "yes");
}

// ========== Practical combined ==========

#[test]
fn test_practical_null_safe_default() {
    assert_eq!(
        run_php(
            "<?php
$config = null;
$value = isset($config) ? $config : 'default';
echo $value;
"
        ),
        "default"
    );
}

#[test]
fn test_practical_type_check_pattern() {
    assert_eq!(
        run_php(
            "<?php
$items = [1, 'two', 3, 'four', 5];
$sum = 0;
foreach ($items as $v) {
    if (is_int($v)) {
        $sum += $v;
    }
}
echo $sum;
"
        ),
        "9"
    );
}

#[test]
fn test_practical_isset_with_unset_loop() {
    assert_eq!(
        run_php(
            "<?php
$a = 1; $b = 2; $c = 3;
unset($b);
$result = '';
if (isset($a)) { $result .= 'a'; }
if (isset($b)) { $result .= 'b'; }
if (isset($c)) { $result .= 'c'; }
echo $result;
"
        ),
        "ac"
    );
}

#[test]
fn test_practical_cast_sum_strings() {
    assert_eq!(
        run_php(
            "<?php
$a = '10';
$b = '20';
echo (int)$a + (int)$b;
"
        ),
        "30"
    );
}

// ========== empty() with expressions ==========

#[test]
fn test_empty_expression_truthy() {
    assert_eq!(
        run_php("<?php $x = 1; echo empty($x + 1) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_empty_expression_falsy() {
    assert_eq!(
        run_php("<?php $x = 0; echo empty($x + 0) ? 'yes' : 'no';"),
        "yes"
    );
}

// ========== unset() on non-array ==========

#[test]
fn test_unset_dim_on_scalar_fatal() {
    let result = std::panic::catch_unwind(|| run_php("<?php $x = 42; unset($x[0]);"));
    assert!(result.is_err());
}

#[test]
fn test_unset_dim_on_null_silent() {
    assert_eq!(run_php("<?php $x = null; unset($x[0]); echo 'ok';"), "ok");
}

#[test]
fn test_unset_dim_on_undef_silent() {
    assert_eq!(run_php("<?php unset($x[0]); echo 'ok';"), "ok");
}
