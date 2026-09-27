mod common;

use common::run_php;

#[test]
fn posix_extension_is_honestly_admitted_with_its_implemented_surface() {
    let output = run_php(
        r#"<?php
var_dump(extension_loaded('posix'));
var_dump(in_array('posix', get_loaded_extensions(), true));
$functions = get_extension_funcs('posix');
sort($functions);
echo implode(',', $functions), "\n";
"#,
    );
    assert_eq!(
        output,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "posix_errno,posix_get_last_error,posix_geteuid,posix_getpwuid,",
            "posix_getuid,posix_isatty,posix_strerror\n",
        )
    );
}

#[test]
fn phpunit_posix_functions_expose_php_85_reflection() {
    let output = run_php(
        r#"<?php
foreach (['posix_getuid', 'posix_geteuid', 'posix_get_last_error', 'posix_errno', 'posix_strerror', 'posix_getpwuid', 'posix_isatty'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '|', $function->getExtensionName(), '|',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), '|',
        (string) $function->getReturnType();
    foreach ($function->getParameters() as $parameter) {
        echo '|', $parameter->getName(), ':',
            $parameter->hasType() ? (string) $parameter->getType() : 'none';
    }
    echo "\n";
}
"#,
    );
    assert_eq!(
        output,
        concat!(
            "posix_getuid|posix|0/0|int\n",
            "posix_geteuid|posix|0/0|int\n",
            "posix_get_last_error|posix|0/0|int\n",
            "posix_errno|posix|0/0|int\n",
            "posix_strerror|posix|1/1|string|error_code:int\n",
            "posix_getpwuid|posix|1/1|array|false|user_id:int\n",
            "posix_isatty|posix|1/1|bool|file_descriptor:none\n",
        )
    );
}

#[test]
fn posix_isatty_retains_errno_for_the_error_api() {
    let output = run_php(
        r#"<?php
var_dump(posix_get_last_error(), posix_errno());
var_dump(posix_isatty(10024));
var_dump(posix_get_last_error(), posix_errno(), posix_strerror(posix_errno()));
set_error_handler(function ($level, $message) { echo 'D:', $message, "\n"; });
var_dump(posix_isatty(-1), posix_strerror(posix_get_last_error()));
"#,
    );
    assert_eq!(
        output,
        concat!(
            "int(0)\n",
            "int(0)\n",
            "bool(false)\n",
            "int(9)\n",
            "int(9)\n",
            "string(19) \"Bad file descriptor\"\n",
            "D:posix_isatty(): Argument #1 ($file_descriptor) must be between 0 and 2147483647\n",
            "bool(false)\n",
            "string(19) \"Bad file descriptor\"\n",
        )
    );
}

#[test]
fn effective_user_projects_the_native_password_record() {
    let output = run_php(
        r#"<?php
$uid = posix_geteuid();
$record = posix_getpwuid($uid);
echo get_debug_type($uid), '|', get_debug_type($record), '|',
    implode(',', array_keys($record)), '|',
    implode(',', array_map('get_debug_type', array_values($record))), '|',
    (int) ($record['uid'] === $uid), "\n";
var_dump(posix_getpwuid(-99));
"#,
    );
    assert_eq!(
        output,
        "int|array|name,passwd,uid,gid,gecos,dir,shell|string,string,int,int,string,string,string|1\nbool(false)\n"
    );
}

#[test]
fn posix_isatty_matches_weak_manual_zpp_and_stream_projection() {
    let output = run_php(
        r#"<?php
set_error_handler(function ($level, $message) { echo 'D:', $message, "\n"; });
foreach ([null, false, true, 1, 1.0, 5.5, '1', '1.0', '5.5', 'Hello', [], new stdClass] as $value) {
    echo get_debug_type($value), ':';
    var_dump(posix_isatty($value));
}
restore_error_handler();
var_dump(posix_isatty(STDIN));
"#,
    );
    assert_eq!(
        output,
        concat!(
            "null:D:posix_isatty(): Passing null to parameter #1 ($file_descriptor) of type int is deprecated\n",
            "bool(false)\n",
            "bool:bool(false)\n",
            "bool:bool(false)\n",
            "int:bool(false)\n",
            "float:bool(false)\n",
            "float:D:Implicit conversion from float 5.5 to int loses precision\n",
            "bool(false)\n",
            "string:bool(false)\n",
            "string:bool(false)\n",
            "string:D:Implicit conversion from float-string \"5.5\" to int loses precision\n",
            "bool(false)\n",
            "string:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, string given\n",
            "bool(false)\n",
            "array:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, array given\n",
            "bool(false)\n",
            "stdClass:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, stdClass given\n",
            "bool(false)\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn posix_isatty_strict_calls_warn_and_return_false_without_coercion() {
    let output = run_php(
        r#"<?php declare(strict_types=1);
set_error_handler(function ($level, $message) { echo 'D:', $message, "\n"; });
foreach ([null, false, true, 1.0, '1', []] as $value) {
    echo get_debug_type($value), ':';
    var_dump(posix_isatty($value));
}
"#,
    );
    assert_eq!(
        output,
        concat!(
            "null:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, null given\n",
            "bool(false)\n",
            "bool:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, false given\n",
            "bool(false)\n",
            "bool:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, true given\n",
            "bool(false)\n",
            "float:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, float given\n",
            "bool(false)\n",
            "string:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, string given\n",
            "bool(false)\n",
            "array:D:posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, array given\n",
            "bool(false)\n",
        )
    );
}
