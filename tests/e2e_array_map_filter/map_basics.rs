// ============================================================
// array_map
// ============================================================

#[test]
fn test_array_map_basic() {
    let out = run_php(
        r#"<?php
function double($x) { return $x * 2; }
$arr = [1, 2, 3];
$result = array_map("double", $arr);
echo $result[0] . "," . $result[1] . "," . $result[2];
"#,
    );
    assert_eq!(out, "2,4,6");
}

#[test]
fn test_array_map_strings() {
    let out = run_php(
        r#"<?php
function upper($s) { return strtoupper($s); }
$arr = ["hello", "world"];
$result = array_map("upper", $arr);
echo $result[0] . " " . $result[1];
"#,
    );
    assert_eq!(out, "HELLO WORLD");
}

#[test]
fn test_array_map_preserves_keys() {
    let out = run_php(
        r#"<?php
function inc($x) { return $x + 1; }
$arr = ["a" => 10, "b" => 20];
$result = array_map("inc", $arr);
echo $result["a"] . "," . $result["b"];
"#,
    );
    assert_eq!(out, "11,21");
}

#[test]
fn test_array_map_supports_general_callable_forms() {
    let out = run_php(
        r#"<?php
class MapCallbacks {
    public function triple($value) { return $value * 3; }
    public static function addTen($value) { return $value + 10; }
    public function __invoke($value) { return $value - 1; }
}
$offset = 4;
$closure = function($value) use ($offset) { return $value + $offset; };
$callbacks = new MapCallbacks();
$closureResult = array_map($closure, [1, 2]);
$methodResult = array_map([$callbacks, "triple"], [2, 3]);
$staticResult = array_map(["MapCallbacks", "addTen"], [4, 5]);
$invokeResult = array_map($callbacks, [7, 8]);
echo $closureResult[0] . "," . $closureResult[1] . ":";
echo $methodResult[0] . "," . $methodResult[1] . ":";
echo $staticResult[0] . "," . $staticResult[1] . ":";
echo $invokeResult[0] . "," . $invokeResult[1];
"#,
    );
    assert_eq!(out, "5,6:6,9:14,15:6,7");
}

#[test]
fn test_array_map_with_stdlib_callback() {
    let out = run_php(
        r#"<?php
$arr = ["hello", "world"];
$result = array_map("strlen", $arr);
echo $result[0] . "," . $result[1];
"#,
    );
    assert_eq!(out, "5,5");
}

#[test]
fn test_array_map_closure() {
    let out = run_php(
        r#"<?php
function square($n) { return $n * $n; }
$nums = [1, 2, 3, 4];
$squares = array_map("square", $nums);
echo array_sum($squares);
"#,
    );
    assert_eq!(out, "30");
}

#[test]
fn test_array_map_empty() {
    let out = run_php(
        r#"<?php
function double($x) { return $x * 2; }
$result = array_map("double", []);
echo count($result);
"#,
    );
    assert_eq!(out, "0");
}
