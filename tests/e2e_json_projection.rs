mod common;

use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::opcode::OpCode;

fn main_opcodes(source: &str) -> Vec<OpCode> {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new()
        .compile(&statements)
        .unwrap()
        .main
        .instructions
        .iter()
        .map(|instruction| instruction.opcode)
        .collect()
}

#[test]
fn associative_decode_uses_binary_direct_abi_but_default_decode_does_not() {
    let associative = main_opcodes("<?php json_decode('{}', true);");
    assert!(associative.contains(&OpCode::DirectInternalCall2));
    assert!(!associative.contains(&OpCode::DoFcall));

    let default_object = main_opcodes("<?php json_decode('{}');");
    assert!(!default_object.contains(&OpCode::DirectInternalCall2));
    assert!(default_object.contains(&OpCode::DoFcall));
}

#[test]
fn callback_decode_uses_the_shared_frame_free_handler_for_both_arities() {
    assert_eq!(
        run_php(
            r#"<?php
$array = call_user_func('json_decode', '{"value":7}', true);
$object = call_user_func('json_decode', '{"value":9}');
echo $array['value'] . '|' . $object->value;
"#,
        ),
        "7|9"
    );
}

#[test]
fn hot_invariant_json_projection_preserves_result_after_loop() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"name":"Alice","age":30,"scores":[95,87]}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['age'] + $row['scores'][0] + $row['scores'][1];
}
echo $sum . '|' . $row['name'] . '|' . $row['scores'][1];
"#,
        ),
        "21200|Alice|87"
    );
}

#[test]
fn invariant_string_projection_derives_length_once_and_preserves_the_leaf() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"name":"hyper-optimized"}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + strlen($row['name']);
}
echo $sum . '|' . $row['name'];
"#,
        ),
        "1500|hyper-optimized"
    );
}

#[test]
fn invariant_double_projection_feeds_the_existing_scalar_call_plan() {
    assert_eq!(
        run_php(
            r#"<?php
function scaleJsonProjection(float $value): float {
    return $value * 1.5;
}
$json = '{"value":1.25}';
$sum = 0.0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum += scaleJsonProjection($row['value']);
}
echo $sum . '|' . $row['value'];
"#,
        ),
        "187.5|1.25"
    );
}

#[test]
fn non_double_call_projection_falls_back_to_php_coercion() {
    assert_eq!(
        run_php(
            r#"<?php
function scaleJsonCoercion(float $value): float {
    return $value * 1.5;
}
$json = '{"value":2}';
$sum = 0.0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum += scaleJsonCoercion($row['value']);
}
echo $sum . '|' . $row['value'];
"#,
        ),
        "300|2"
    );
}

#[test]
fn non_string_length_projection_falls_back_to_canonical_strlen() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"name":123}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + strlen($row['name']);
}
echo $sum . '|' . $row['name'];
"#,
        ),
        "300|123"
    );
}

#[test]
fn literal_json_input_uses_the_same_projection_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode('{"left":1,"nested":{"right":2}}', true);
    $sum = $sum + $row['left'] + $row['nested']['right'];
}
echo $sum . '|' . $row['nested']['right'];
"#,
        ),
        "300|2"
    );
}

#[test]
fn projection_uses_canonical_numeric_string_array_keys() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '[7,11]';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['0'] + $row['1'];
}
echo $sum;
"#,
        ),
        "1800"
    );
}

#[test]
fn non_long_projection_falls_back_before_mutating_the_frame() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"value":1.25}';
$sum = 0.0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['value'];
}
echo $sum . '|' . $row['value'];
"#,
        ),
        "125|1.25"
    );
}

#[test]
fn missing_long_projection_preserves_canonical_null_arithmetic() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"other":10}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['value'];
}
echo $sum;
"#,
        ),
        "0"
    );
}

#[test]
fn json_input_modified_in_loop_is_not_hoisted() {
    assert_eq!(
        run_php(
            r#"<?php
$first = '{"value":1}';
$second = '{"value":2}';
$json = $first;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    if (($i % 2) == 0) {
        $json = $first;
    } else {
        $json = $second;
    }
    $row = json_decode($json, true);
    $sum = $sum + $row['value'];
}
echo $sum;
"#,
        ),
        "150"
    );
}

#[test]
fn conditionally_unreachable_decode_is_not_published_by_the_prelude() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"value":1}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    if ($i < 0) {
        $row = json_decode($json, true);
        $sum = $sum + $row['value'];
    } else {
        $sum = $sum + 1;
    }
}
echo $sum . '|' . isset($row);
"#,
        ),
        "100|"
    );
}

#[test]
fn decoded_array_mutation_is_not_reused_across_iterations() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"value":1}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $row['value'] = $row['value'] + 1;
    $sum = $sum + $row['value'];
}
echo $sum . '|' . $row['value'];
"#,
        ),
        "200|2"
    );
}

#[test]
fn decoded_array_append_is_not_reused_across_iterations() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"value":1}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $row[] = 5;
    $sum = $sum + $row['value'];
}
echo $sum . '|' . count($row);
"#,
        ),
        "100|2"
    );
}

#[test]
fn default_object_decode_keeps_canonical_path() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"value":7}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json);
    $sum = $sum + $row->value;
}
echo $sum;
"#,
        ),
        "700"
    );
}
