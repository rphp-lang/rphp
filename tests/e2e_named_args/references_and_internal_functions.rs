// ── P1 regression: by-ref named arguments ──

#[test]
fn test_named_args_by_ref() {
    assert_eq!(
        run_php(
            r#"<?php
function inc(&$x) { $x = $x + 1; }
$a = 1;
inc(x: $a);
echo $a;
"#
        ),
        "2"
    );
}

#[test]
fn test_named_args_by_ref_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public function bump(&$val) { $val++; }
}
$c = new Counter();
$x = 10;
$c->bump(val: $x);
echo $x;
"#
        ),
        "11"
    );
}

// ── P1 regression: named args skip required param ──

#[test]
fn test_named_args_skip_required_fatal() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
function f($a, $b) { echo $a; }
f(b: 2);
"#,
        );
    });
    assert!(result.is_err());
}

#[test]
fn test_named_args_skip_required_constructor() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
class C {
    public function __construct($a, $b) {}
}
new C(b: 2);
"#,
        );
    });
    assert!(result.is_err());
}

// ── P1 regression: named args with internal functions ──

#[test]
fn test_named_args_strlen_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo strlen(string: "abc");
"#
        ),
        "3"
    );
}

#[test]
fn test_named_args_substr_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo substr(string: "abcdef", offset: 2, length: 3);
"#
        ),
        "cde"
    );
}

#[test]
fn test_named_args_exception_constructor() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Exception(message: "test error");
echo $e->getMessage();
"#
        ),
        "test error"
    );
}

#[test]
fn test_exception_constructor_code_and_previous() {
    assert_eq!(
        run_php(
            r#"<?php
$previous = new Exception("first", 7);
$error = new RuntimeException("second", 42, $previous);
echo $error->getMessage(), ':', $error->getCode(), ':', $error->getPrevious()->getMessage();
"#
        ),
        "second:42:first"
    );
}

#[test]
fn test_named_args_str_replace_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo str_replace(search: "world", replace: "PHP", subject: "hello world");
"#
        ),
        "hello PHP"
    );
}

#[test]
fn test_named_args_in_array_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo in_array(needle: 3, haystack: [1,2,3]) ? "yes" : "no";
"#
        ),
        "yes"
    );
}
