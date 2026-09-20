mod common;

use common::run_php;

#[test]
fn reference_foreach_tracks_internal_shift_and_unshift_splices() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['a', 'b', 'c'];
foreach ($array as $key => &$value) {
    echo "$key:$value;";
    array_shift($array);
}
unset($value);
echo '|', json_encode($array), "\n";

$array = ['a', 'b', 'c'];
foreach ($array as $key => &$value) {
    echo "$key:$value;";
    array_unshift($array, 'n');
    if ($value === 'c') break;
}
unset($value);
echo '|', json_encode($array);
"#,
        ),
        concat!(
            "0:a;0:b;0:c;|[]\n",
            "0:a;2:b;4:c;|[\"n\",\"n\",\"n\",\"a\",\"b\",\"c\"]",
        )
    );
}

#[test]
fn ordinary_object_foreach_observes_live_declared_and_dynamic_properties() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class MutableProperties {
    public $a = 'a';
    public $b = 'b';
    public $c = 'c';
}
$object = new MutableProperties();
foreach ($object as $key => $value) {
    echo "$key:$value;";
    if ($key === 'a') {
        unset($object->c);
        $object->d = 'd';
    }
    if ($key === 'b') unset($object->a);
}
echo '|', json_encode($object);
"#,
        ),
        "a:a;b:b;d:d;|{\"b\":\"b\",\"d\":\"d\"}"
    );
}

#[test]
fn reference_foreach_reports_a_source_replaced_with_a_scalar() {
    assert_eq!(
        run_php(
            r#"<?php
$array = [42];
set_error_handler(function ($severity, $message) {
    echo $severity, ':', $message, '|';
    return true;
});
foreach ($array as &$value) {
    unset($array[0]);
    $array = null;
}
unset($value);
echo 'done';
"#,
        ),
        "2:foreach() argument must be of type array|object, null given|done"
    );
}

#[test]
fn scalar_foreach_diagnostics_preserve_boolean_value_names() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo $message, '|';
    return true;
});
foreach (true as $value) {}
foreach (false as $value) {}
"#,
        ),
        concat!(
            "foreach() argument must be of type array|object, true given|",
            "foreach() argument must be of type array|object, false given|",
        )
    );
}

#[test]
fn traversable_argument_unpack_canonicalizes_decimal_string_keys() {
    assert_eq!(
        run_php(
            r#"<?php
function unpackSource() {
    yield '100' => 'a';
    yield '101' => 'b';
    yield '102' => 'c';
    yield 'named' => 'd';
}
function receive($first = null, $second = null, ...$rest) {
    echo $first, ',', $second, ',', json_encode($rest);
}
receive(...unpackSource());
"#,
        ),
        "a,b,{\"0\":\"c\",\"named\":\"d\"}"
    );
}

#[test]
fn value_specific_runtime_diagnostics_cover_false_and_enum_cases() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    false?->method();
} catch (Error $error) {
    echo $error->getMessage(), '|';
}

enum IterationState {
    case Ready;
    case Done;
}
try {
    match (IterationState::Ready) {
        IterationState::Done => 1,
    };
} catch (UnhandledMatchError $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "Call to a member function method() on false|",
            "Unhandled match case IterationState::Ready",
        )
    );
}

#[test]
fn array_reduce_reports_hard_reference_parameters_for_each_callback() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo strstr($message, 'Argument'), '|';
    return true;
});
$result = array_reduce([1, 2], function (&$carry, &$item) {
    return $carry + $item;
}, 0);
echo $result;
"#,
        ),
        concat!(
            "Argument #1 ($carry) must be passed by reference, value given|",
            "Argument #2 ($item) must be passed by reference, value given|",
            "Argument #1 ($carry) must be passed by reference, value given|",
            "Argument #2 ($item) must be passed by reference, value given|3",
        )
    );
}
