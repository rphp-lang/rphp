// -- call_user_func_array callback and argument shapes --

#[test]
fn test_call_user_func_array_function_packed_args() {
    let out = run_php(
        r#"<?php
function add_array_args($a, $b) { return $a + $b; }
echo call_user_func_array('add_array_args', [3, 7]);
"#,
    );
    assert_eq!(out, "10");
}

#[test]
fn test_direct_internal_callback_matches_hash_frame_fallback() {
    let out = run_php(
        r#"<?php
$packed_args = ['abcd', 2, '|'];
$packed = call_user_func_array('chunk_split', $packed_args);
$hash = call_user_func_array('chunk_split', [4 => 'abcd', 8 => 2, 12 => '|']);
echo $packed . ':' . $hash;
"#,
    );
    assert_eq!(out, "ab|cd|:ab|cd|");
}

#[test]
fn test_direct_internal_callback_preserves_scalar_coercion() {
    let out = run_php(
        r#"<?php
$strlen_args = [12345];
echo call_user_func_array('strlen', $strlen_args);
echo ':';
$abs_args = [-7];
echo call_user_func_array('abs', $abs_args);
"#,
    );
    assert_eq!(out, "5:7");
}

#[test]
fn test_direct_binary_internal_callback_uses_shared_abi() {
    assert_eq!(
        run_php("<?php echo call_user_func_array('intdiv', ['9', 2]);"),
        "4",
    );
}

#[test]
fn test_call_user_func_array_prebuilt_and_literal_use_distinct_lowerings() {
    let prebuilt = main_opcodes("<?php $args = ['abc']; call_user_func_array('strlen', $args);");
    assert!(prebuilt.contains(&OpCode::CallUserFuncArray));

    let literal = main_opcodes("<?php call_user_func_array('strlen', ['abc']);");
    assert!(literal.contains(&OpCode::InitUserCall));
    assert!(literal.contains(&OpCode::SendUserChecked));
    assert!(!literal.contains(&OpCode::CallUserFuncArray));
}

#[test]
fn test_one_arg_callback_site_mixes_direct_and_framed_results() {
    let out = run_php(
        r#"<?php
function callback_passthrough($value) { return '<' . $value . '>'; }
$callbacks = ['strtoupper', 'callback_passthrough', 'strlen', 'strtolower'];
foreach ($callbacks as $callback) {
    echo call_user_func_array($callback, ['AbC']);
    echo ':';
}
echo call_user_func('ord', 'Z');
"#,
    );
    assert_eq!(out, "ABC:<AbC>:3:abc:90");
}

#[test]
fn test_lowered_callback_calls_preserve_argument_evaluation_order() {
    let out = run_php(
        r#"<?php
function make_callback() { echo 'C'; return 'strlen'; }
function make_argument() { echo 'A'; return 'abc'; }
echo call_user_func(make_callback(), make_argument());
echo ':';
echo call_user_func_array(make_callback(), [make_argument()]);
"#,
    );
    assert_eq!(out, "CA3:CA3");
}

#[test]
fn test_namespaced_shadow_keeps_normal_function_call() {
    let out = run_php(
        r#"<?php
namespace CallbackShadow {
    function call_user_func_array($callback, $args) { return 'shadow'; }
    echo call_user_func_array('strlen', ['abc']);
}
"#,
    );
    assert_eq!(out, "shadow");
}

#[test]
fn test_lowered_call_user_func_keeps_by_value_semantics() {
    let out = run_php(
        r#"<?php
function bump_callback(&$value) { $value = $value + 1; }
$value = 1;
set_error_handler(function($_severity, $message) { echo "warning:$message|"; });
call_user_func('bump_callback', $value);
restore_error_handler();
echo $value;
"#,
    );
    assert_eq!(
        out,
        "warning:bump_callback(): Argument #1 ($value) must be passed by reference, value given|1"
    );
}

#[test]
fn test_call_user_func_array_closure_with_capture() {
    let out = run_php(
        r#"<?php
$factor = 4;
$multiply = function($value) use ($factor) { return $value * $factor; };
echo call_user_func_array($multiply, [6]);
"#,
    );
    assert_eq!(out, "24");
}

#[test]
fn test_call_user_func_array_instance_and_static_methods() {
    let out = run_php(
        r#"<?php
class CallbackMath {
    public function add($a, $b) { return $a + $b; }
    public static function multiply($a, $b) { return $a * $b; }
}
$math = new CallbackMath();
echo call_user_func_array([$math, 'add'], [2, 5]);
echo ':';
echo call_user_func_array(['CallbackMath', 'multiply'], [3, 4]);
"#,
    );
    assert_eq!(out, "7:12");
}

#[test]
fn test_call_user_func_array_invokable_object() {
    let out = run_php(
        r#"<?php
class InvokableCallback {
    public function __invoke($value) { return $value * 5; }
}
$callback = new InvokableCallback();
echo call_user_func_array($callback, [3]);
"#,
    );
    assert_eq!(out, "15");
}

#[test]
fn test_call_user_func_array_trait_method() {
    let out = run_php(
        r#"<?php
trait CallbackTrait {
    public function triple($value) { return $value * 3; }
}
class TraitCallback {
    use CallbackTrait;
}
$callback = new TraitCallback();
echo call_user_func_array([$callback, 'triple'], [4]);
"#,
    );
    assert_eq!(out, "12");
}

#[test]
fn test_call_user_func_array_named_method_args() {
    let out = run_php(
        r#"<?php
class NamedCallback {
    public function format($left, $right) { return $left . ':' . $right; }
}
$callback = new NamedCallback();
echo call_user_func_array([$callback, 'format'], ['right' => 'R', 'left' => 'L']);
"#,
    );
    assert_eq!(out, "L:R");
}

#[test]
fn test_call_user_func_array_sparse_integer_keys_stay_positional() {
    let out = run_php(
        r#"<?php
function sparse_args($first, $second) { return $first . ':' . $second; }
echo call_user_func_array('sparse_args', [7 => 'A', 11 => 'B']);
"#,
    );
    assert_eq!(out, "A:B");
}

#[test]
fn test_call_user_func_array_exception_cleans_callback_frame() {
    let out = run_php(
        r#"<?php
function callback_boom($value) { throw new Exception('boom'); }
function callback_ok($value) { return $value + 1; }
try {
    call_user_func_array('callback_boom', [1]);
} catch (Throwable $e) {
    echo 'caught:';
}
echo call_user_func_array('callback_ok', [4]);
"#,
    );
    assert_eq!(out, "caught:5");
}

#[test]
fn test_call_user_func_array_cache_tracks_mutated_callback_name() {
    let out = run_php(
        r#"<?php
function callback_a($value) { return 'a' . $value; }
function callback_ab($value) { return 'ab' . $value; }
function invoke_at_one_site($callback, $value) {
    return call_user_func_array($callback, [$value]);
}
$callback = 'callback_a';
echo invoke_at_one_site($callback, 1);
$callback .= 'b';
echo ':';
echo invoke_at_one_site($callback, 2);
$callback = 'callback_a';
echo ':';
echo invoke_at_one_site($callback, 3);
"#,
    );
    assert_eq!(out, "a1:ab2:a3");
}

#[test]
fn test_call_user_func_array_same_closure_body_uses_current_captures() {
    let out = run_php(
        r#"<?php
function make_adder($amount) {
    return function($value) use ($amount) { return $value + $amount; };
}
function invoke_closure_at_one_site($callback, $value) {
    return call_user_func_array($callback, [$value]);
}
$add_two = make_adder(2);
$add_ten = make_adder(10);
echo invoke_closure_at_one_site($add_two, 5);
echo ':';
echo invoke_closure_at_one_site($add_ten, 5);
"#,
    );
    assert_eq!(out, "7:15");
}

#[test]
fn test_call_user_func_array_writes_fresh_heap_result_slot() {
    let out = run_php(
        r#"<?php
function callback_object($value) {
    $result = new stdClass();
    $result->value = $value;
    return $result;
}
function invoke_object_callback($callback, $value) {
    $padding = [new stdClass(), new stdClass(), new stdClass()];
    return call_user_func_array($callback, [$value]);
}
for ($i = 0; $i < 25; $i++) {
    $result = invoke_object_callback('callback_object', $i);
}
echo $result->value;
"#,
    );
    assert_eq!(out, "24");
}

#[test]
fn test_array_walk_method_reads_back_first_public_argument() {
    let out = run_php(
        r#"<?php
class Walker {
    public function bump(&$value, $key) { $value += 10; }
}
$walker = new Walker();
$values = [1, 2];
array_walk($values, [$walker, 'bump']);
echo implode(',', $values);
"#,
    );
    assert_eq!(out, "11,12");
}

#[test]
fn test_array_walk_scalar_callback_replays_non_long_and_impure_inputs() {
    let out = run_php(
        r#"<?php
function scalarWalk($value, $key) { return $value * 3 + $key; }
function runScalarWalk(&$values) { return array_walk($values, "scalarWalk"); }
$longs = [1, 2, 3];
echo (runScalarWalk($longs) ? "1" : "0") . ":" . implode(",", $longs) . "|";
$doubles = [1.5, 2.5];
echo (runScalarWalk($doubles) ? "1" : "0") . ":" . implode(",", $doubles) . "|";
function orderedWalk($value, $key) { echo "v" . $value . "k" . $key; }
$ordered = [4, 5];
array_walk($ordered, "orderedWalk");
"#,
    );
    assert_eq!(out, "1:1,2,3|1:1.5,2.5|v4k0v5k1");
}

#[test]
fn test_usort_scalar_callback_replays_non_long_overflow_and_impure_inputs() {
    let out = run_php(
        r#"<?php
function compareLongs($left, $right) { return $left - $right; }
function runScalarSort(&$values) { return usort($values, "compareLongs"); }
$longs = [3, 1, 2];
echo (runScalarSort($longs) ? "1" : "0") . ":" . implode(",", $longs) . "|";
$doubles = [3.5, 1.5, 2.5];
echo (runScalarSort($doubles) ? "1" : "0") . ":" . implode(",", $doubles) . "|";
$max = 9223372036854775807;
$overflow = [$max, -$max];
echo (runScalarSort($overflow) ? "1" : "0") . ":" . implode(",", $overflow) . "|";
function compareLongsDescending($left, $right) { return $right - $left; }
$descending = [1, 3, 2];
echo (usort($descending, "compareLongsDescending") ? "1" : "0") . ":" . implode(",", $descending) . "|";
function compareSpaceship($left, $right) { return $left <=> $right; }
$spaceship = [3, 1, 2];
echo (usort($spaceship, "compareSpaceship") ? "1" : "0") . ":" . implode(",", $spaceship) . "|";
function orderedSort($left, $right) { echo $left . $right; return $left - $right; }
$ordered = [3, 1, 2];
usort($ordered, "orderedSort");
echo ":" . implode(",", $ordered);
"#,
    );
    assert_eq!(
        out,
        "1:1,2,3|1:1.5,2.5,3.5|1:-9223372036854775807,9223372036854775807|1:3,2,1|1:1,2,3|312132:1,2,3"
    );
}

#[test]
fn test_first_class_method_callable_works_with_array_map() {
    let out = run_php(
        r#"<?php
class Formatter {
    public function format($value) { return strtoupper($value); }
}
$formatter = new Formatter();
echo implode(',', array_map($formatter->format(...), ['a', 'b']));
"#,
    );
    assert_eq!(out, "A,B");
}

#[test]
fn test_first_class_method_callable_is_a_typed_closure() {
    let out = run_php(
        r#"<?php
class TypedCallable {
    private \Closure $callback;
    public function __construct() { $this->callback = $this->format(...); }
    private function format($value) { return strtoupper($value); }
    public function run() { return ($this->callback)('ok'); }
}
echo (new TypedCallable())->run();
"#,
    );
    assert_eq!(out, "OK");
}

#[test]
fn test_first_class_static_method_callable_works_with_array_map() {
    let out = run_php(
        r#"<?php
class StaticFormatter {
    public static function format($value) { return strtoupper($value); }
    public static function run() { return array_map(self::format(...), ['a', 'b']); }
}
echo implode(',', StaticFormatter::run());
"#,
    );
    assert_eq!(out, "A,B");
}

#[test]
fn test_first_class_function_callable_uses_namespace_fallback() {
    let out = run_php(
        r#"<?php
namespace App;
$check = match ('int') {
    'int' => is_int(...),
    default => is_string(...),
};
echo ($check(1) ? '1' : '0') . ($check('1') ? '1' : '0');
"#,
    );
    assert_eq!(out, "10");
}

#[test]
fn test_dynamic_class_name_static_method_call() {
    let out = run_php(
        r#"<?php
class DynamicFactory {
    public static function create($value) { return strtoupper($value); }
}
$class = DynamicFactory::class;
echo $class::create('dynamic');
"#,
    );
    assert_eq!(out, "DYNAMIC");
}

#[test]
fn test_dynamic_object_method_expression_call() {
    let out = run_php(
        r#"<?php
class DynamicHandler {
    public function format($value) { return strtoupper($value); }
}
$handler = new DynamicHandler();
$methods = ['service' => 'format'];
$method = $methods['service'];
echo $handler->{$method}('rphp'), '|', $handler->$method('kernel');
"#,
    );
    assert_eq!(out, "RPHP|KERNEL");
}

#[test]
fn test_dynamic_static_call_preserves_late_static_scope() {
    let out = run_php(
        r#"<?php
class DynamicBase {
    public const VALUE = 'base';
    public static function value() { return static::VALUE; }
}
class DynamicChild extends DynamicBase { public const VALUE = 'child'; }
$class = DynamicChild::class;
echo $class::value();
"#,
    );
    assert_eq!(out, "child");
}

#[test]
fn test_forwarding_self_call_preserves_late_static_scope() {
    let out = run_php(
        r#"<?php
class ForwardingBase {
    public const VALUE = 'base';
    public static function outer() { return self::inner(); }
    private static function inner() { return static::VALUE; }
}
class ForwardingChild extends ForwardingBase { public const VALUE = 'child'; }
echo ForwardingChild::outer();
"#,
    );
    assert_eq!(out, "child");
}
