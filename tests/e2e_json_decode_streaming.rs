mod common;

use common::run_php;

#[test]
fn streaming_decode_preserves_scalars_and_number_tags() {
    assert_eq!(
        run_php(
            r#"<?php
$v = json_decode('{"null":null,"true":true,"false":false,"int":-7,"float":1.25e2,"big":9223372036854775808}', true);
echo (is_null($v['null']) ? 'N' : 'x')
    . ($v['true'] ? 'T' : 'x')
    . (!$v['false'] ? 'F' : 'x')
    . '|' . $v['int'] . '|' . $v['float']
    . '|' . (is_float($v['big']) ? 'float' : 'not-float');
"#,
        ),
        "NTF|-7|125|float"
    );
}

#[test]
fn streaming_decode_preserves_escapes_and_unicode_surrogates() {
    assert_eq!(
        run_php(
            r#"<?php
$v = json_decode('{"escaped":"line\nquote\"slash\\\\","unicode":"\uD83D\uDE00"}', true);
echo $v['escaped'] . '|' . $v['unicode'];
"#,
        ),
        "line\nquote\"slash\\|😀"
    );
}

#[test]
fn streaming_decode_builds_nested_arrays_and_objects_directly() {
    assert_eq!(
        run_php(
            r#"<?php
$array = json_decode('{"nested":{"x":7},"items":[{"y":9},2]}', true);
$object = json_decode('{"nested":{"x":7},"items":[{"y":9},2]}');
echo $array['nested']['x'] . '|' . $array['items'][0]['y']
    . '|' . $object->nested->x . '|' . $object->items[0]->y;
"#,
        ),
        "7|9|7|9"
    );
}

#[test]
fn streaming_decode_keeps_last_duplicate_value_and_first_position() {
    assert_eq!(
        run_php(
            r#"<?php
$v = json_decode('{"b":1,"a":2,"b":3}', true);
foreach ($v as $key => $value) {
    echo $key . ':' . $value . '|';
}
"#,
        ),
        "b:3|a:2|"
    );
}

#[test]
fn streaming_decode_normalizes_only_canonical_numeric_array_keys() {
    assert_eq!(
        run_php(
            r#"<?php
$list = json_decode('{"0":"zero"}', true);
$mixed = json_decode('{"0":"zero","01":"leading","-2":"negative","9223372036854775808":"huge"}', true);
echo json_encode($list) . '|' . $mixed[0] . '|' . $mixed['01']
    . '|' . $mixed[-2] . '|' . $mixed['9223372036854775808'];
"#,
        ),
        "[\"zero\"]|zero|leading|negative|huge"
    );
}

#[test]
fn streaming_decode_rejects_invalid_and_trailing_documents() {
    assert_eq!(
        run_php(
            r#"<?php
$a = json_decode('{"x":]');
$b = json_decode('{"x":1} trailing');
$c = json_decode('"\uD800"');
echo (is_null($a) ? 'N' : 'x') . (is_null($b) ? 'N' : 'x') . (is_null($c) ? 'N' : 'x');
"#,
        ),
        "NNN"
    );
}

#[test]
fn streaming_decode_accepts_whitespace_around_one_document() {
    assert_eq!(
        run_php(
            r#"<?php
$v = json_decode("  \n\t [1,2,3] \r\n ", true);
echo count($v) . '|' . $v[2];
"#,
        ),
        "3|3"
    );
}

#[test]
fn decoded_linear_hash_preserves_cow_updates_removal_and_order() {
    assert_eq!(
        run_php(
            r#"<?php
$source = json_decode('{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8}', true);
$copy = $source;
$copy['d'] = 40;
unset($copy['b']);
$copy['i'] = 9;
echo $source['b'] . '|' . $source['d'] . '|' . count($source)
    . '|' . (isset($copy['b']) ? 'bad' : 'missing')
    . '|' . $copy['d'] . '|' . $copy['i'] . '|' . count($copy) . '|';
foreach ($copy as $key => $value) {
    echo $key;
}
"#,
        ),
        "2|4|8|missing|40|9|8|acdefghi"
    );
}

#[test]
fn decoded_stdclass_property_cache_guards_receiver_shape_and_missing_keys() {
    assert_eq!(
        run_php(
            r#"<?php
class DeclaredRow {
    public $value = 31;
}
function value_of($row) {
    return $row->value;
}
$first = json_decode('{"value":11}');
$second = json_decode('{"value":17}');
$declared = new DeclaredRow();
$missing = json_decode('{"other":1}');
echo value_of($first) . '|' . value_of($second) . '|'
    . value_of($declared) . '|' . value_of($first) . '|';
$first->value = 23;
echo value_of($first) . '|'
    . (value_of($missing) === null ? 'null' : 'bad');
"#,
        ),
        "11|17|31|11|23|null"
    );
}

#[test]
fn decoded_stdclass_preserves_duplicate_position_and_property_order() {
    assert_eq!(
        run_php(
            r#"<?php
$value = json_decode('{"a":1,"b":2,"a":3,"c":4}');
echo $value->a . '|' . json_encode($value);
"#,
        ),
        "3|{\"a\":3,\"b\":2,\"c\":4}"
    );
}

#[test]
fn property_strlen_fusion_guards_dynamic_declared_and_missing_receivers() {
    assert_eq!(
        run_php(
            r#"<?php
class NamedValue {
    public $name = 'declared';
}
function name_length($value) {
    return strlen($value->name);
}
$dynamic = json_decode('{"name":"alpha"}');
$declared = new NamedValue();
$missing = json_decode('{"other":"value"}');
echo name_length($dynamic) . '|'
    . name_length($dynamic) . '|'
    . name_length($declared) . '|'
    . name_length($dynamic) . '|'
    . name_length($missing);
"#,
        ),
        "5|5|8|5|0"
    );
}

#[test]
fn typed_property_read_region_rebinds_each_receiver_and_property_order() {
    assert_eq!(
        run_php(
            r#"<?php
class DeclaredRow {
    public $name = 'gamma';
    public $value = 3;
}
function summarize($row) {
    $sum = 0;
    for ($i = 0; $i < 200; $i++) {
        $sum += $row->value + strlen($row->name);
    }
    return $sum;
}
$first = json_decode('{"value":11,"name":"alpha"}');
$second = json_decode('{"name":"beta","value":7}');
echo summarize($first) . '|'
    . summarize($second) . '|'
    . summarize(new DeclaredRow()) . '|'
    . summarize($first);
"#,
        ),
        "3200|2200|1600|3200"
    );
}
