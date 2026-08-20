// ============================================================
// array_filter
// ============================================================

#[test]
fn test_array_filter_with_callback() {
    let out = run_php(
        r#"<?php
function is_even($x) { return $x % 2 == 0; }
$arr = [1, 2, 3, 4, 5, 6];
$result = array_filter($arr, "is_even");
echo count($result);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_array_filter_without_callback() {
    // Filter by truthiness — removes 0, "", null, false
    let out = run_php(
        r#"<?php
$arr = [0, 1, "", "hello", null, 42];
$result = array_filter($arr);
echo count($result);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_array_filter_preserves_keys() {
    let out = run_php(
        r#"<?php
function gt_two($x) { return $x > 2; }
$arr = [1, 2, 3, 4];
$result = array_filter($arr, "gt_two");
$keys = array_keys($result);
echo $keys[0] . "," . $keys[1];
"#,
    );
    assert_eq!(out, "2,3");
}

#[test]
fn test_array_filter_supports_closure_and_method_callbacks() {
    let out = run_php(
        r#"<?php
class FilterCallbacks {
    public function keepOdd($value) { return $value & 1; }
}
$minimum = 2;
$closure = function($value) use ($minimum) { return $value > $minimum; };
$callbacks = new FilterCallbacks();
$closureResult = array_filter([1, 2, 3, 4], $closure);
$methodResult = array_filter([1, 2, 3, 4], [$callbacks, "keepOdd"]);
echo count($closureResult) . ":" . $closureResult[2] . "," . $closureResult[3] . ":";
echo count($methodResult) . ":" . $methodResult[0] . "," . $methodResult[2];
"#,
    );
    assert_eq!(out, "2:3,4:2:1,3");
}

#[test]
fn test_array_reduce_scalar_and_general_callbacks() {
    let out = run_php(
        r#"<?php
function sumValues($carry, $value) { return $carry + $value; }
$factor = 2;
$closure = function($carry, $value) use ($factor) { return $carry + $value * $factor; };
echo array_reduce([1, 2, 3, 4], "sumValues", 0) . ":";
echo array_reduce([1, 2, 3, 4], $closure, 0);
"#,
    );
    assert_eq!(out, "10:20");
}

#[test]
fn test_nested_scalar_callback_pipeline_preserves_result() {
    let out = run_php(
        r#"<?php
function pipelineMap($value) { return $value * 3 + 1; }
function pipelineKeep($value) { return $value & 1; }
function pipelineSum($carry, $value) { return $carry + $value; }
$result = array_reduce(
    array_filter(array_map("pipelineMap", [0, 1, 2, 3, 4, 5]), "pipelineKeep"),
    "pipelineSum",
    0
);
echo $result;
"#,
    );
    assert_eq!(out, "21");
}

#[test]
fn test_nested_scalar_callback_pipeline_falls_back_for_double_input() {
    let out = run_php(
        r#"<?php
function fallbackMap($value) { return $value + 1; }
function fallbackKeep($value) { return $value; }
function fallbackSum($carry, $value) { return $carry + $value; }
function runPipeline($values) {
    return array_reduce(
        array_filter(array_map("fallbackMap", $values), "fallbackKeep"),
        "fallbackSum",
        0
    );
}
echo runPipeline([1, 2]) . ":";
$doubleResult = runPipeline([1.5, 2.5]);
echo gettype($doubleResult) . ":" . $doubleResult;
"#,
    );
    assert_eq!(out, "5:double:6");
}

#[test]
fn test_nested_callback_pipeline_keeps_canonical_callback_order() {
    let out = run_php(
        r#"<?php
function orderedMap($value) { echo "m" . $value; return $value + 1; }
function orderedKeep($value) { echo "f" . $value; return $value & 1; }
function orderedSum($carry, $value) { echo "r" . $value; return $carry + $value; }
$result = array_reduce(
    array_filter(array_map("orderedMap", [1, 2, 3]), "orderedKeep"),
    "orderedSum",
    0
);
echo ":" . $result;
"#,
    );
    assert_eq!(out, "m1m2m3f2f3f4r3:3");
}

#[test]
fn test_nested_scalar_callback_pipeline_replays_overflow_canonically() {
    let out = run_php(
        r#"<?php
function overflowMap($value) { return $value + 1; }
function overflowKeep($value) { return 1; }
function overflowSum($carry, $value) { return $carry + $value; }
$result = array_reduce(
    array_filter(array_map("overflowMap", [9223372036854775807]), "overflowKeep"),
    "overflowSum",
    0
);
echo gettype($result);
"#,
    );
    assert_eq!(out, "double");
}

#[test]
fn test_dead_staged_scalar_callback_pipeline_preserves_result() {
    let out = run_php(
        r#"<?php
function stagedMap($value) { return $value * 3 + 1; }
function stagedKeep($value) { return $value & 1; }
function stagedSum($carry, $value) { return $carry + $value; }
function stagedPipeline($values) {
    $mapped = array_map("stagedMap", $values);
    $filtered = array_filter($mapped, "stagedKeep");
    return array_reduce($filtered, "stagedSum", 0);
}
echo stagedPipeline([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "21");
}

#[test]
fn test_escaping_staged_callback_pipeline_materializes_results() {
    let out = run_php(
        r#"<?php
function escapingMap($value) { return $value * 3 + 1; }
function escapingKeep($value) { return $value & 1; }
function escapingSum($carry, $value) { return $carry + $value; }
function escapingPipeline($values) {
    $mapped = array_map("escapingMap", $values);
    $filtered = array_filter($mapped, "escapingKeep");
    $sum = array_reduce($filtered, "escapingSum", 0);
    return count($mapped) . ":" . count($filtered) . ":" . $sum;
}
echo escapingPipeline([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "6:3:21");
}

#[test]
fn test_initialized_staged_destination_uses_canonical_assignment() {
    let out = run_php(
        r#"<?php
function initializedMap($value) { return $value * 3 + 1; }
function initializedKeep($value) { return $value & 1; }
function initializedSum($carry, $value) { return $carry + $value; }
function initializedPipeline($values, &$mapped) {
    $mapped = array_map("initializedMap", $values);
    $filtered = array_filter($mapped, "initializedKeep");
    return array_reduce($filtered, "initializedSum", 0);
}
$initialized = 99;
$first = initializedPipeline([0, 1, 2, 3, 4, 5], $initialized);
$second = initializedPipeline([0, 1, 2, 3, 4, 5], $undefined);
echo gettype($initialized) . ":" . count($initialized) . ":" . $first . "|";
echo gettype($undefined) . ":" . count($undefined) . ":" . $second;
"#,
    );
    assert_eq!(out, "array:6:21|array:6:21");
}

#[test]
fn test_filter_map_reduce_pipeline_preserves_nested_and_staged_results() {
    let out = run_php(
        r#"<?php
function filterMapKeep($value) { return $value & 1; }
function filterMapMap($value) { return $value * 3 + 1; }
function filterMapSum($carry, $value) { return $carry + $value; }
function nestedFilterMap($values) {
    return array_reduce(
        array_map("filterMapMap", array_filter($values, "filterMapKeep")),
        "filterMapSum",
        0
    );
}
function stagedFilterMap($values) {
    $filtered = array_filter($values, "filterMapKeep");
    $mapped = array_map("filterMapMap", $filtered);
    return array_reduce($mapped, "filterMapSum", 0);
}
echo nestedFilterMap([0, 1, 2, 3, 4, 5]) . ":";
echo stagedFilterMap([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "30:30");
}

#[test]
fn test_filter_map_pipeline_keeps_canonical_impure_order() {
    let out = run_php(
        r#"<?php
function orderedFilterMapKeep($value) { echo "f" . $value; return $value & 1; }
function orderedFilterMapMap($value) { echo "m" . $value; return $value * 3 + 1; }
function orderedFilterMapSum($carry, $value) { echo "r" . $value; return $carry + $value; }
$result = array_reduce(
    array_map("orderedFilterMapMap", array_filter([0, 1, 2, 3, 4, 5], "orderedFilterMapKeep")),
    "orderedFilterMapSum",
    0
);
echo ":" . $result;
"#,
    );
    assert_eq!(out, "f0f1f2f3f4f5m1m3m5r4r10r16:30");
}

#[test]
fn test_filter_map_pipeline_replays_double_input_canonically() {
    let out = run_php(
        r#"<?php
function doubleFilterMapKeep($value) { return 1; }
function doubleFilterMapMap($value) { return $value + 1; }
function doubleFilterMapSum($carry, $value) { return $carry + $value; }
function doubleFilterMapPipeline($values) {
    return array_reduce(
        array_map("doubleFilterMapMap", array_filter($values, "doubleFilterMapKeep")),
        "doubleFilterMapSum",
        0
    );
}
echo doubleFilterMapPipeline([1, 2]) . ":";
$double = doubleFilterMapPipeline([1.5, 2.5]);
echo gettype($double) . ":" . $double;
"#,
    );
    assert_eq!(out, "5:double:6");
}

#[test]
fn test_filter_map_staged_escape_and_reference_destination_materialize() {
    let out = run_php(
        r#"<?php
function materializedFilterMapKeep($value) { return $value & 1; }
function materializedFilterMapMap($value) { return $value * 3 + 1; }
function materializedFilterMapSum($carry, $value) { return $carry + $value; }
function escapingFilterMap($values) {
    $filtered = array_filter($values, "materializedFilterMapKeep");
    $mapped = array_map("materializedFilterMapMap", $filtered);
    $sum = array_reduce($mapped, "materializedFilterMapSum", 0);
    return count($filtered) . ":" . count($mapped) . ":" . $sum;
}
function referencedFilterMap($values, &$filtered) {
    $filtered = array_filter($values, "materializedFilterMapKeep");
    $mapped = array_map("materializedFilterMapMap", $filtered);
    return array_reduce($mapped, "materializedFilterMapSum", 0);
}
echo escapingFilterMap([0, 1, 2, 3, 4, 5]) . "|";
$sum = referencedFilterMap([0, 1, 2, 3, 4, 5], $external);
echo gettype($external) . ":" . count($external) . ":" . $sum;
"#,
    );
    assert_eq!(out, "3:3:30|array:3:30");
}

#[test]
fn test_json_callback_pipeline_preserves_all_admitted_shapes() {
    let out = run_php(
        r#"<?php
function jsonPipelineMap($value) { return $value * 3 + 1; }
function jsonPipelineKeep($value) { return $value & 1; }
function jsonPipelineSum($carry, $value) { return $carry + $value; }
function nestedJsonMapFilter($values) {
    return json_encode(array_reduce(
        array_filter(array_map("jsonPipelineMap", $values), "jsonPipelineKeep"),
        "jsonPipelineSum",
        0
    ));
}
function stagedJsonMapFilter($values) {
    $mapped = array_map("jsonPipelineMap", $values);
    $filtered = array_filter($mapped, "jsonPipelineKeep");
    return json_encode(array_reduce($filtered, "jsonPipelineSum", 0));
}
function nestedJsonFilterMap($values) {
    return json_encode(array_reduce(
        array_map("jsonPipelineMap", array_filter($values, "jsonPipelineKeep")),
        "jsonPipelineSum",
        0
    ));
}
function stagedJsonFilterMap($values) {
    $filtered = array_filter($values, "jsonPipelineKeep");
    $mapped = array_map("jsonPipelineMap", $filtered);
    return json_encode(array_reduce($mapped, "jsonPipelineSum", 0));
}
$values = [0, 1, 2, 3, 4, 5];
echo nestedJsonMapFilter($values) . ":";
echo stagedJsonMapFilter($values) . ":";
echo nestedJsonFilterMap($values) . ":";
echo stagedJsonFilterMap($values);
"#,
    );
    assert_eq!(out, "21:21:30:30");
}

#[test]
fn test_json_callback_pipeline_falls_back_for_double_and_impure_callbacks() {
    let out = run_php(
        r#"<?php
function jsonFallbackMap($value) { return $value + 1; }
function jsonFallbackKeep($value) { return 1; }
function jsonFallbackSum($carry, $value) { return $carry + $value; }
function jsonFallbackPipeline($values) {
    return json_encode(array_reduce(
        array_filter(array_map("jsonFallbackMap", $values), "jsonFallbackKeep"),
        "jsonFallbackSum",
        0
    ));
}
echo jsonFallbackPipeline([1, 2]) . ":";
echo jsonFallbackPipeline([1.5, 2.5]) . "|";
function jsonOrderedMap($value) { echo "m" . $value; return $value + 1; }
function jsonOrderedKeep($value) { echo "f" . $value; return $value & 1; }
function jsonOrderedSum($carry, $value) { echo "r" . $value; return $carry + $value; }
$result = json_encode(array_reduce(
    array_filter(array_map("jsonOrderedMap", [1, 2, 3]), "jsonOrderedKeep"),
    "jsonOrderedSum",
    0
));
echo ":" . $result;
"#,
    );
    assert_eq!(out, "5:6|m1m2m3f2f3f4r3:3");
}

#[test]
fn test_json_staged_pipeline_materializes_escaping_intermediates() {
    let out = run_php(
        r#"<?php
function jsonEscapingMap($value) { return $value * 3 + 1; }
function jsonEscapingKeep($value) { return $value & 1; }
function jsonEscapingSum($carry, $value) { return $carry + $value; }
function jsonEscapingPipeline($values) {
    $mapped = array_map("jsonEscapingMap", $values);
    $filtered = array_filter($mapped, "jsonEscapingKeep");
    $encoded = json_encode(array_reduce($filtered, "jsonEscapingSum", 0));
    return count($mapped) . ":" . count($filtered) . ":" . $encoded;
}
echo jsonEscapingPipeline([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "6:3:21");
}

#[test]
fn test_json_pipeline_respects_namespaced_json_encode_shadow() {
    let out = run_php(
        r#"<?php
namespace PipelineJsonShadow;
function json_encode($value) { return "custom:" . $value; }
function shadowMap($value) { return $value * 3 + 1; }
function shadowKeep($value) { return $value & 1; }
function shadowSum($carry, $value) { return $carry + $value; }
echo json_encode(array_reduce(
    array_filter(
        array_map("PipelineJsonShadow\\shadowMap", [0, 1, 2, 3, 4, 5]),
        "PipelineJsonShadow\\shadowKeep"
    ),
    "PipelineJsonShadow\\shadowSum",
    0
));
"#,
    );
    assert_eq!(out, "custom:21");
}

#[test]
fn test_array_filter_all_pass() {
    let out = run_php(
        r#"<?php
function is_positive($x) { return $x > 0; }
$arr = [1, 2, 3];
$result = array_filter($arr, "is_positive");
echo count($result);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_array_filter_none_pass() {
    let out = run_php(
        r#"<?php
function is_negative($x) { return $x < 0; }
$arr = [1, 2, 3];
$result = array_filter($arr, "is_negative");
echo count($result);
"#,
    );
    assert_eq!(out, "0");
}

#[test]
fn test_array_filter_empty() {
    let out = run_php(
        r#"<?php
function always_true($x) { return true; }
$result = array_filter([], "always_true");
echo count($result);
"#,
    );
    assert_eq!(out, "0");
}
