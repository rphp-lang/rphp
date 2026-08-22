use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

fn assert_run(source: &str, stdout: &str) {
    assert_eq!(run_stdin(source), (0, stdout.to_string(), String::new()));
}

#[test]
fn classifiers_distinguish_finite_infinite_and_nan_values() {
    assert_run(
        r#"<?php
var_dump(M_PI);
foreach ([NAN, INF, -INF, 0.0, 1] as $number) {
    echo is_nan($number) ? 'nan' : '-', ',';
    echo is_finite($number) ? 'finite' : '-', ',';
    echo is_infinite($number) ? 'infinite' : '-', "\n";
}
"#,
        concat!(
            "float(3.141592653589793)\n",
            "nan,-,-\n",
            "-,-,infinite\n",
            "-,-,infinite\n",
            "-,finite,-\n",
            "-,finite,-\n",
        ),
    );
}

#[test]
fn weak_classifiers_accept_php_numeric_scalars_and_deprecate_null() {
    assert_run(
        r#"<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
foreach (['is_nan', 'is_finite', 'is_infinite'] as $function) {
    var_dump($function(null));
}
var_dump(is_finite(false), is_finite(true), is_finite(' 1.5 '));
var_dump(is_infinite('1e999'), is_nan('1e999'));
"#,
        concat!(
            "8192:is_nan(): Passing null to parameter #1 ($num) of type float is deprecated\n",
            "bool(false)\n",
            "8192:is_finite(): Passing null to parameter #1 ($num) of type float is deprecated\n",
            "bool(true)\n",
            "8192:is_infinite(): Passing null to parameter #1 ($num) of type float is deprecated\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
        ),
    );
}

#[test]
fn invalid_weak_values_report_the_exact_parameter_contract() {
    assert_run(
        r#"<?php
foreach ([
    ['is_nan', 'NAN'],
    ['is_finite', []],
    ['is_infinite', new stdClass],
] as [$function, $value]) {
    try { $function($value); } catch (Throwable $error) {
        echo $error::class, ': ', $error->getMessage(), "\n";
    }
}
"#,
        concat!(
            "TypeError: is_nan(): Argument #1 ($num) must be of type float, string given\n",
            "TypeError: is_finite(): Argument #1 ($num) must be of type float, array given\n",
            "TypeError: is_infinite(): Argument #1 ($num) must be of type float, stdClass given\n",
        ),
    );
}

#[test]
fn strict_classifiers_only_widen_integers() {
    assert_run(
        r#"<?php
declare(strict_types=1);
var_dump(is_finite(1), is_nan(1.0));
foreach ([false, '1.5', null] as $value) {
    try { is_infinite($value); } catch (Throwable $error) {
        echo $error->getMessage(), "\n";
    }
}
"#,
        concat!(
            "bool(true)\n",
            "bool(false)\n",
            "is_infinite(): Argument #1 ($num) must be of type float, false given\n",
            "is_infinite(): Argument #1 ($num) must be of type float, string given\n",
            "is_infinite(): Argument #1 ($num) must be of type float, null given\n",
        ),
    );
}

#[test]
fn namespaced_and_first_class_calls_use_the_same_internal_contract() {
    assert_run(
        r#"<?php
namespace Classification;
$nan = \is_nan(...);
var_dump($nan(NAN), is_finite(2), is_infinite(INF));
"#,
        "bool(true)\nbool(true)\nbool(true)\n",
    );
}
