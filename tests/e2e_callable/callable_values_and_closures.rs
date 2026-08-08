// -- is_callable with string --

#[test]
fn test_is_callable_existing_function() {
    let out = run_php(
        r#"<?php
echo is_callable('strtoupper') ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_nonexistent_function() {
    let out = run_php(
        r#"<?php
echo is_callable('nonexistent_func') ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_user_function() {
    let out = run_php(
        r#"<?php
function myFunc() {}
echo is_callable('myFunc') ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "true");
}

// -- is_callable with non-callable values --

#[test]
fn test_is_callable_integer() {
    let out = run_php(
        r#"<?php
echo is_callable(42) ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_null() {
    let out = run_php(
        r#"<?php
echo is_callable(null) ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "false");
}

// -- call_user_func with closure --

#[test]
fn test_call_user_func_closure() {
    let out = run_php(
        r#"<?php
$fn = function($x) { return $x * 3; };
echo call_user_func($fn, 4);
"#,
    );
    assert_eq!(out, "12");
}

#[test]
fn test_lowered_closure_call_keeps_optional_gap_before_capture() {
    let out = run_php(
        r#"<?php
$prefix = 'P';
$format = function($value, $suffix = '!') use ($prefix) {
    return $prefix . $value . $suffix;
};
echo call_user_func($format, 'x');
echo ':';
echo call_user_func_array($format, ['y']);
"#,
    );
    assert_eq!(out, "Px!:Py!");
}

#[test]
fn test_is_callable_closure() {
    let out = run_php(
        r#"<?php
$fn = function() { return 1; };
echo is_callable($fn) ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "true");
}
