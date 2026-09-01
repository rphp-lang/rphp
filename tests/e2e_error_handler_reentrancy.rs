mod common;

use common::run_php;

#[test]
fn active_error_handler_is_temporarily_hidden_and_a_nested_handler_may_run() {
    assert_eq!(
        run_php(
            r#"<?php
$events = [];
$outer = function () use (&$events) {
    $events[] = 'outer';
    var_dump(get_error_handler());
    set_error_handler(function () use (&$events) { $events[] = 'inner'; });
    $inside++;
    restore_error_handler();
};
set_error_handler($outer);
$outside++;
var_dump(get_error_handler() === $outer);
echo implode(',', $events), "\n";
"#,
        ),
        "NULL\nbool(true)\nouter,inner\n"
    );
}

#[test]
fn captured_handler_retains_undeclared_public_arguments_for_introspection() {
    assert_eq!(
        run_php(
            r#"<?php
$seen = [];
set_error_handler(function ($level) use (&$seen) {
    $args = func_get_args();
    $seen = [count($args), $args[0], is_string($args[1]), is_string($args[2]), $args[3] > 0];
});
$missing++;
var_dump($seen);
"#,
        ),
        concat!(
            "array(5) {\n",
            "  [0]=>\n  int(4)\n",
            "  [1]=>\n  int(2)\n",
            "  [2]=>\n  bool(true)\n",
            "  [3]=>\n  bool(true)\n",
            "  [4]=>\n  bool(true)\n",
            "}\n",
        )
    );
}

#[test]
fn restoring_a_previous_handler_inside_the_active_handler_is_observable() {
    assert_eq!(
        run_php(
            r#"<?php
$first = function () { echo "first\n"; };
$second = function () {
    echo "second\n";
    restore_error_handler();
    trigger_error('inside', E_USER_WARNING);
};
set_error_handler($first);
set_error_handler($second);
$outside++;
var_dump(get_error_handler() === $first);
"#,
        ),
        "second\nfirst\nbool(true)\n"
    );
}

#[test]
fn missing_stream_warnings_enter_the_handler_and_may_throw() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { throw new Exception($message); });
try {
    fopen('/rphp/missing/error-handler-oracle', 'r');
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
try {
    require_once '/rphp/missing/error-handler-oracle.php';
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "fopen(/rphp/missing/error-handler-oracle): Failed to open stream: No such file or directory\n",
            "require_once(/rphp/missing/error-handler-oracle.php): Failed to open stream: No such file or directory\n",
        )
    );
}

#[test]
fn compound_array_write_does_not_overwrite_a_handler_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
$array = [];
set_error_handler(function () use (&$array) { $array['slot'] = 12; });
$array['slot'] += 3;
var_dump($array);
"#,
        ),
        "array(1) {\n  [\"slot\"]=>\n  int(12)\n}\n"
    );
}

#[test]
fn target_root_clobber_prevents_direct_and_compound_recreation() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function () { $GLOBALS['array'] = null; });
$array[$missing] = 'x';
var_dump($array);
$array[$missing] .= 'y';
var_dump($array);
$nullKey = ['keep' => 1];
set_error_handler(function () { $GLOBALS['nullKey'] = null; });
$nullKey[null] = 'z';
var_dump($nullKey);
"#,
        ),
        "NULL\nNULL\nNULL\n"
    );
}

#[test]
fn string_offset_read_uses_the_pre_handler_value_but_preserves_the_handler_write() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function () { $GLOBALS['text'] = 99; });
$text = 'abc';
var_dump($text[$missing]);
var_dump($text);
"#,
        ),
        "string(1) \"a\"\nint(99)\n"
    );
}

#[test]
fn nested_string_offset_write_does_not_republish_a_stale_byte_buffer() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $message, "\n";
    $GLOBALS['text'] = '';
});
$text = ['abc'];
$text[0][$missing] = 'z';
var_dump($text);
"#,
        ),
        concat!(
            "Undefined variable $missing\n",
            "String offset cast occurred\n",
            "string(0) \"\"\n",
        )
    );
}

#[test]
fn key_conversion_diagnostic_does_not_republish_a_clobbered_array() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $message, "\n";
    $GLOBALS['target'] = [];
});
$target = ['keep' => 1];
$target[1e20] = 2;
var_dump($target);

function localTargetOracle() {
    $target = null;
    set_error_handler(function ($level, $message) use (&$target) {
        echo $message, "\n";
        $target = ['handler' => 3];
    });
    $target[1e20] = 2;
    var_dump($target);
}
localTargetOracle();
"#,
        ),
        concat!(
            "The float 1.0E+20 is not representable as an int, cast occurred\n",
            "array(0) {\n}\n",
            "The float 1.0E+20 is not representable as an int, cast occurred\n",
            "array(1) {\n  [\"handler\"]=>\n  int(3)\n}\n",
        )
    );
}

#[test]
fn detached_handler_observes_and_may_capture_the_live_main_scope_binding() {
    assert_eq!(
        run_php(
            r#"<?php
$source = ['live' => 7];
$captured = null;
set_error_handler(function () {
    $GLOBALS['captured'] = $GLOBALS['source'];
});
function provoke(&$slot) {
    $slot = null;
    $slot[1e20] = 'engine';
}
provoke($source);
var_dump($source, $captured);
"#,
        ),
        "array(0) {\n}\narray(0) {\n}\n"
    );
}

#[test]
fn property_compound_write_continues_from_the_value_installed_by_the_handler() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class Counter {
    function onError($level, $message) {
        echo $message, "\n";
        $this->value = 12345;
    }
}
$counter = new Counter;
set_error_handler([$counter, 'onError']);
$counter->value %= 10;
var_dump($counter->value);
"#,
        ),
        "Undefined property: Counter::$value\nint(5)\n"
    );
}

#[test]
fn array_cow_and_reference_targets_abort_only_when_the_target_was_mutated() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['keep' => 1];
$copy = $array;
set_error_handler(function () use (&$array) { $array['keep'] = 2; });
$array['missing'] += 3;
var_dump($array, $copy);

$array = [];
$other = 0;
set_error_handler(function () use (&$other) { $other = 7; });
$array['missing'] += 3;
var_dump($array, $other);
"#,
        ),
        concat!(
            "array(1) {\n  [\"keep\"]=>\n  int(2)\n}\n",
            "array(1) {\n  [\"keep\"]=>\n  int(1)\n}\n",
            "array(1) {\n  [\"missing\"]=>\n  int(3)\n}\n",
            "int(7)\n",
        )
    );
}

#[test]
fn ordinary_key_side_effects_still_use_the_live_container() {
    assert_eq!(
        run_php(
            r#"<?php
$array = [];
function oracleKey() {
    global $array;
    $array = [0 => 8];
    return 0;
}
$array[oracleKey()] += 2;
var_dump($array);
"#,
        ),
        "array(1) {\n  [0]=>\n  int(10)\n}\n"
    );
}
