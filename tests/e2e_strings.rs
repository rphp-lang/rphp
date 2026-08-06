/// E2E tests: strings, concatenation, escapes, UTF-8, truthiness.
mod common;
use common::run_php;

// === String basics ===

#[test]
fn test_e2e_echo_string() {
    assert_eq!(run_php("<?php echo \"hello\";"), "hello");
}

#[test]
fn test_e2e_echo_single_quoted() {
    assert_eq!(run_php("<?php echo 'world';"), "world");
}

#[test]
fn test_e2e_concat_strings() {
    assert_eq!(
        run_php("<?php echo \"hello\" . \" \" . \"world\";"),
        "hello world"
    );
}

#[test]
fn test_e2e_concat_string_int() {
    assert_eq!(run_php("<?php echo \"value: \" . 42;"), "value: 42");
}

#[test]
fn test_e2e_concat_variable() {
    assert_eq!(
        run_php("<?php $name = \"PHP\"; echo \"Hello \" . $name;"),
        "Hello PHP"
    );
}

#[test]
fn test_e2e_string_assign_echo() {
    assert_eq!(run_php("<?php $s = \"test\"; echo $s;"), "test");
}

// === Concat precedence ===

#[test]
fn test_e2e_concat_plus_precedence() {
    assert_eq!(run_php("<?php echo \"x\" . 1 + 2;"), "x3");
}

#[test]
fn test_e2e_concat_mul_precedence() {
    assert_eq!(run_php("<?php echo \"val\" . 3 * 4;"), "val12");
}

// === String concat in loop (tests Drop correctness) ===

#[test]
fn test_e2e_concat_in_loop() {
    assert_eq!(
        run_php(
            "<?php $s = \"\"; $i = 0; while ($i < 3) { $s = $s . \"x\"; $i = $i + 1; } echo $s;"
        ),
        "xxx"
    );
}

// === Reassign string variable (tests Drop on overwrite) ===

#[test]
fn test_e2e_string_reassign() {
    assert_eq!(
        run_php("<?php $s = \"hello\"; $s = \"world\"; echo $s;"),
        "world"
    );
}

// === UTF-8 ===

#[test]
fn test_e2e_utf8_string() {
    assert_eq!(run_php("<?php echo \"Ahoj světe\";"), "Ahoj světe");
}

#[test]
fn test_e2e_utf8_concat() {
    assert_eq!(run_php("<?php echo \"Č\" . \"esky\";"), "Česky");
}

// === String truthiness ===

#[test]
fn test_e2e_string_truthy() {
    assert_eq!(run_php("<?php if (\"hello\") echo 1;"), "1");
}

#[test]
fn test_e2e_empty_string_falsy() {
    assert_eq!(run_php("<?php if (\"\") echo 1;"), "");
}

#[test]
fn test_e2e_string_zero_falsy() {
    assert_eq!(run_php("<?php if (\"0\") echo 1;"), "");
}

// === Escape sequences ===

#[test]
fn test_e2e_double_quote_newline() {
    assert_eq!(run_php("<?php echo \"a\\nb\";"), "a\nb");
}

#[test]
fn test_e2e_double_quote_tab() {
    assert_eq!(run_php("<?php echo \"a\\tb\";"), "a\tb");
}

#[test]
fn test_e2e_double_quote_escaped_backslash() {
    assert_eq!(run_php("<?php echo \"a\\\\b\";"), "a\\b");
}

#[test]
fn test_e2e_double_quote_escaped_dollar() {
    assert_eq!(run_php("<?php echo \"a\\$b\";"), "a$b");
}

#[test]
fn test_e2e_double_quote_escaped_quote() {
    assert_eq!(run_php(r#"<?php echo "a\"b";"#), "a\"b");
}

#[test]
fn test_e2e_single_quote_literal_backslash_n() {
    assert_eq!(run_php("<?php echo 'a\\nb';"), "a\\nb");
}

#[test]
fn test_e2e_single_quote_escaped_backslash() {
    assert_eq!(run_php("<?php echo 'a\\\\b';"), "a\\b");
}

#[test]
fn test_e2e_single_quote_escaped_quote() {
    assert_eq!(run_php("<?php echo 'a\\'b';"), "a'b");
}

// ========== String interpolation ==========

#[test]
fn test_string_interpolation_basic() {
    assert_eq!(
        run_php("<?php $name = 'World'; echo \"Hello $name\";"),
        "Hello World"
    );
}

#[test]
fn test_string_interpolation_multiple_vars() {
    assert_eq!(
        run_php("<?php $a = 'foo'; $b = 'bar'; echo \"$a and $b\";"),
        "foo and bar"
    );
}

#[test]
fn test_string_interpolation_with_number() {
    assert_eq!(
        run_php("<?php $n = 42; echo \"The answer is $n\";"),
        "The answer is 42"
    );
}

#[test]
fn test_string_interpolation_escaped_dollar() {
    assert_eq!(run_php("<?php $x = 5; echo \"Cost: \\$x\";"), "Cost: $x");
}

#[test]
fn test_string_interpolation_curly_brace() {
    assert_eq!(
        run_php("<?php $fruit = 'banana'; echo \"I like {$fruit}s\";"),
        "I like bananas"
    );
}

#[test]
fn test_string_interpolation_no_vars() {
    assert_eq!(run_php("<?php echo \"just a string\";"), "just a string");
}

#[test]
fn test_string_interpolation_only_var() {
    assert_eq!(run_php("<?php $x = 'hello'; echo \"$x\";"), "hello");
}

#[test]
fn test_string_interpolation_with_newline() {
    assert_eq!(run_php("<?php $n = 'Bob'; echo \"Hi $n\\n\";"), "Hi Bob\n");
}

#[test]
fn test_single_quote_no_interpolation() {
    assert_eq!(run_php("<?php $x = 5; echo '$x';"), "$x");
}

#[test]
fn test_interpolation_array_int_index() {
    assert_eq!(
        run_php("<?php $a = ['hello', 'world']; echo \"val: {$a[0]}\";"),
        "val: hello"
    );
}

#[test]
fn test_interpolation_array_string_key() {
    assert_eq!(
        run_php("<?php $m = ['name' => 'PHP']; echo \"lang: {$m['name']}\";"),
        "lang: PHP"
    );
}

#[test]
fn test_practical_string_interpolation_in_loop() {
    assert_eq!(
        run_php(
            "<?php
$names = ['Alice', 'Bob'];
foreach ($names as $name) {
    echo \"Hello $name! \";
}
"
        ),
        "Hello Alice! Hello Bob! "
    );
}

// ── COW string aliasing regression tests ──────────────────────────

#[test]
fn test_string_cow_assign_then_mutate() {
    // $b = $a shares Rc. $b .= must COW-detach, not mutate $a.
    assert_eq!(
        run_php(
            "<?php
$a = 'hello';
$b = $a;
$b .= ' world';
echo $a . '|' . $b;
"
        ),
        "hello|hello world"
    );
}

#[test]
fn test_string_cow_function_arg() {
    // Function arg is a clone (Rc bump). .= inside must not affect caller.
    assert_eq!(
        run_php(
            "<?php
function modify($s) { $s .= '!'; return $s; }
$x = 'test';
$y = modify($x);
echo $x . '|' . $y;
"
        ),
        "test|test!"
    );
}

#[test]
fn test_string_cow_multiple_clones() {
    // Multiple clones from same source — each .= independent.
    assert_eq!(
        run_php(
            "<?php
$s = 'base';
$c1 = $s;
$c2 = $s;
$c3 = $s;
$c1 .= '1';
$c2 .= '2';
echo $s . '|' . $c1 . '|' . $c2 . '|' . $c3;
"
        ),
        "base|base1|base2|base"
    );
}

#[test]
fn test_string_cow_sole_owner_inplace() {
    // Sole owner .= should mutate in place (no COW detach needed).
    assert_eq!(
        run_php(
            "<?php
$z = 'only';
$z .= ' me';
echo $z;
"
        ),
        "only me"
    );
}

#[test]
fn test_string_cow_in_array() {
    // String stored in array, copied out, mutated — original in array unchanged.
    assert_eq!(
        run_php(
            "<?php
$arr = ['key' => 'value'];
$copy = $arr['key'];
$copy .= '_modified';
echo $arr['key'] . '|' . $copy;
"
        ),
        "value|value_modified"
    );
}

#[test]
fn test_string_cow_closure_capture() {
    // Closure captures string. .= inside closure must not affect outer.
    assert_eq!(
        run_php(
            "<?php
$s = 'captured';
$fn = function() use ($s) { $s .= '!'; return $s; };
$r = $fn();
echo $s . '|' . $r;
"
        ),
        "captured|captured!"
    );
}

#[test]
fn test_string_cow_loop_append() {
    // Repeated .= on sole-owner string in a loop.
    assert_eq!(
        run_php(
            "<?php
$s = '';
for ($i = 0; $i < 5; $i = $i + 1) {
    $s .= 'x';
}
echo $s;
"
        ),
        "xxxxx"
    );
}

#[test]
fn test_string_cow_return_and_second_consumer() {
    // Function returns a string, two callers get it — independent copies.
    assert_eq!(
        run_php(
            "<?php
function make() { return 'base'; }
$a = make();
$b = make();
$a .= '1';
$b .= '2';
echo $a . '|' . $b;
"
        ),
        "base1|base2"
    );
}

#[test]
fn test_string_cow_nested_function_passthrough() {
    // String passed through two function calls, mutated at the end.
    assert_eq!(
        run_php(
            "<?php
function inner($s) { $s .= '!'; return $s; }
function outer($s) { return inner($s); }
$x = 'deep';
$y = outer($x);
echo $x . '|' . $y;
"
        ),
        "deep|deep!"
    );
}
