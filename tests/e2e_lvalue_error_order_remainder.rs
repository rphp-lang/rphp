mod common;

use common::run_php;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn reference_property_target_owns_the_undefined_receiver_error() {
    let output = run_php(
        r#"<?php
try {
    $receiver->value =& $source;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}

$name = '';
try {
    $dynamic->{$name} =& $source;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
?>"#,
    );

    assert_eq!(
        output,
        "Attempt to modify property \"value\" on null\nAttempt to modify property \"\" on null\n"
    );
}

#[test]
fn outer_dynamic_property_name_precedes_inner_property_traversal() {
    let output = run_php(
        r#"<?php
$object = 1;
$inner = 1;
$name = null;
var_dump($object->{$inner}->{$name[1]});
?>"#,
    );

    assert_eq!(
        output,
        "\nWarning: Trying to access array offset on null in <main> on line 5\n\nWarning: Attempt to read property \"1\" on int in <main> on line 5\n\nWarning: Attempt to read property \"\" on null in <main> on line 5\nNULL\n"
    );
}

#[test]
fn included_dynamic_assignment_updates_a_local_compiled_variable() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock precedes epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rphp_lvalue_include_{}_{}.php",
        std::process::id(),
        suffix
    ));
    std::fs::write(
        &path,
        "<?php $name = 'created'; $$name = 'local'; echo $created;",
    )
    .expect("write include fixture");

    let source = format!(
        "<?php function probe($path) {{ include $path; echo ':', $created; }} probe('{}');",
        path.display()
    );
    let output = run_php(&source);
    let _ = std::fs::remove_file(path);

    assert_eq!(output, "local:local");
}

#[test]
fn reference_return_type_is_checked_after_finally_mutates_the_cell() {
    let output = run_php(
        r#"<?php
function &invalid_after_finally(): int {
    $value = 0;
    try {
        return $value;
    } finally {
        $value = 'invalid';
    }
}

try {
    $invalid =& invalid_after_finally();
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}

function &coerced_after_finally(): int {
    $value = 0;
    try {
        return $value;
    } finally {
        $value = '7';
    }
}

$coerced =& coerced_after_finally();
var_dump($coerced);
?>"#,
    );

    assert_eq!(
        output,
        "invalid_after_finally(): Return value must be of type int, string returned\nint(7)\n"
    );
}

#[test]
fn compound_variable_variable_resolves_its_name_after_the_rhs() {
    let output = run_php(
        r#"<?php
error_reporting(0);
$name = 'left';
$left = 1;
$right = 10;
function compound_rhs() {
    global $name;
    $name = 'right';
    return 2;
}
$$name += compound_rhs();
var_dump($left, $right);

function historical_reference_shape() {
    $name = 'a';
    $$name .= $$name[++$name] = 'test';
    return $$name;
}
var_dump(historical_reference_shape());
?>"#,
    );

    assert_eq!(output, "int(1)\nint(12)\nstring(9) \"Arraytest\"\n");
}
