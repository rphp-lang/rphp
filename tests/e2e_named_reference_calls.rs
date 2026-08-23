mod common;
use common::run_php;

#[test]
fn named_array_elements_alias_reference_parameters_before_and_after_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['left' => 0, 'right' => 0];
touch_selected(right: $values['right'], left: $values['left']);
echo $values['left'], ':', $values['right'], "\n";

function touch_selected(
    &$left = null, $p01 = null, $p02 = null, $p03 = null,
    $p04 = null, $p05 = null, $p06 = null, $p07 = null,
    $p08 = null, $p09 = null, $p10 = null, $p11 = null,
    $p12 = null, $p13 = null, $p14 = null, $p15 = null,
    $p16 = null, $p17 = null, $p18 = null, $p19 = null,
    $p20 = null, $p21 = null, $p22 = null, $p23 = null,
    $p24 = null, $p25 = null, $p26 = null, $p27 = null,
    $p28 = null, $p29 = null, $p30 = null, $p31 = null,
    &$right = null
) {
    $left += 2;
    $right += 3;
}

$values = ['left' => 0, 'right' => 0];
touch_selected(right: $values['right'], left: $values['left']);
echo $values['left'], ':', $values['right'];
"#,
        ),
        "2:3\n2:3",
    );
}

#[test]
fn named_non_lvalue_reports_reference_error_before_missing_parameter() {
    assert_eq!(
        run_php(
            r#"<?php
function reference_boundary($required, &$target) {}
try { reference_boundary(target: 17); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(); }
"#,
        ),
        "Error:reference_boundary(): Argument #2 ($target) could not be passed by reference",
    );
}

#[test]
fn named_array_element_for_by_value_parameter_stays_detached() {
    assert_eq!(
        run_php(
            r#"<?php
function value_boundary($item) { $item = 99; }
$values = [7];
value_boundary(item: $values[0]);
echo $values[0], ':';
var_dump(ReflectionReference::fromArrayElement($values, 0));
"#,
        ),
        "7:NULL\n",
    );
}

#[test]
fn named_reference_accepts_a_reference_returning_expression() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 4;
function &reference_source() { global $value; return $value; }
function replace_reference(&$target) { $target = 12; }
replace_reference(target: reference_source());
echo $value;
"#,
        ),
        "12",
    );
}
