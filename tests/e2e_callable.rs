mod common;
use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::function::CleanupMode;
use rphp::vm::instruction::OpType;
use rphp::vm::opcode::OpCode;

fn compile_source(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

fn main_opcodes(source: &str) -> Vec<OpCode> {
    compile_source(source)
        .main
        .instructions
        .iter()
        .map(|instruction| instruction.opcode)
        .collect()
}

// -- call_user_func with string callback --

#[test]
fn test_call_user_func_strtoupper() {
    let out = run_php(r#"<?php
echo call_user_func('strtoupper', 'hello');
"#);
    assert_eq!(out, "HELLO");
}

#[test]
fn test_call_user_func_strlen() {
    let out = run_php(r#"<?php
echo call_user_func('strlen', 'abc');
"#);
    assert_eq!(out, "3");
}

#[test]
fn test_call_user_func_user_function() {
    let out = run_php(r#"<?php
function double($x) { return $x * 2; }
echo call_user_func('double', 5);
"#);
    assert_eq!(out, "10");
}

#[test]
fn test_call_user_func_multiple_args() {
    let out = run_php(r#"<?php
function add($a, $b) { return $a + $b; }
echo call_user_func('add', 3, 7);
"#);
    assert_eq!(out, "10");
}

#[test]
fn test_call_user_func_is_lowered_without_wrapper_call() {
    let opcodes = main_opcodes("<?php call_user_func('strlen', 'abc');");
    assert!(opcodes.contains(&OpCode::InitUserCall));
    assert!(opcodes.contains(&OpCode::SendUser));
}

#[test]
fn test_known_unary_builtin_is_lowered_to_frame_free_call() {
    let opcodes = main_opcodes("<?php $value = 'abc'; strlen($value);");
    assert!(opcodes.contains(&OpCode::Strlen_Cv));
    assert!(!opcodes.contains(&OpCode::InitFcall));
    assert!(!opcodes.contains(&OpCode::SendVal));
    assert!(!opcodes.contains(&OpCode::DoFcall));

    let generic = main_opcodes("<?php $value = -7; abs($value);");
    assert!(generic.contains(&OpCode::DirectInternalCall1));
}

#[test]
fn test_known_binary_builtin_is_lowered_to_frame_free_call() {
    let opcodes = main_opcodes("<?php $left = 9; $right = 2; intdiv($left, $right);");
    assert!(opcodes.contains(&OpCode::DirectInternalCall2));
    assert!(!opcodes.contains(&OpCode::InitFcall));
    assert!(!opcodes.contains(&OpCode::SendVal));
    assert!(!opcodes.contains(&OpCode::DoFcall));

    assert_eq!(run_php("<?php echo intdiv('9', 2);"), "4");
    assert_eq!(run_php("<?php echo gettype(intdiv(1, 0));"), "boolean");
}

#[test]
fn test_direct_binary_builtin_preserves_namespace_resolution() {
    let out = run_php(r#"<?php
namespace DirectBinaryShadow {
    function intdiv($left, $right) { return 99; }
    echo intdiv(9, 2);
    echo ':';
    echo \intdiv(9, 2);
}
"#);
    assert_eq!(out, "99:4");
}

#[test]
fn test_discarded_direct_builtin_skips_tmp_result_write() {
    let compiled = compile_source("<?php $value = 'abc'; strlen($value);");
    let direct = compiled
        .main
        .instructions
        .iter()
        .find(|instruction| instruction.opcode == OpCode::Strlen_Cv)
        .unwrap();
    assert_eq!(direct.result_type, OpType::Unused);

    let compiled = compile_source("<?php $length = strlen('abc');");
    let direct = compiled
        .main
        .instructions
        .iter()
        .find(|instruction| instruction.opcode == OpCode::Strlen)
        .unwrap();
    assert_eq!(direct.result_type, OpType::Tmp);
}

#[test]
fn test_discarded_calls_and_loop_updates_skip_tmp_results() {
    let compiled = compile_source("<?php
function value() { return 7; }
for ($i = 0; $i < 3; $i++) { value(); }
$kept = value();
$old = $i++;
");

    let call_results: Vec<_> = compiled
        .main
        .instructions
        .iter()
        .filter(|instruction| instruction.opcode == OpCode::DoFcall)
        .map(|instruction| instruction.result_type)
        .collect();
    assert!(call_results.contains(&OpType::Unused));
    assert!(call_results.contains(&OpType::Tmp));

    let increment_results: Vec<_> = compiled
        .main
        .instructions
        .iter()
        .filter(|instruction| instruction.opcode == OpCode::PostInc)
        .map(|instruction| instruction.result_type)
        .collect();
    assert!(increment_results.contains(&OpType::Unused));
    assert!(increment_results.contains(&OpType::Tmp));
}

#[test]
fn test_direct_builtin_result_kind_controls_frame_cleanup() {
    let scalar = compile_source("<?php function scalar_direct($value) { return strlen($value); }");
    assert_eq!(scalar.functions[0].1.common.plan.cleanup, CleanupMode::SkipScan);

    let binary = compile_source("<?php function binary_direct($left, $right) { return intdiv($left, $right); }");
    assert_eq!(binary.functions[0].1.common.plan.cleanup, CleanupMode::SkipScan);

    let heap = compile_source("<?php function heap_direct($value) { return strtolower($value); }");
    assert_eq!(heap.functions[0].1.common.plan.cleanup, CleanupMode::ScanAll);
}

#[test]
fn test_named_builtin_argument_keeps_regular_call_protocol() {
    let opcodes = main_opcodes("<?php strlen(string: 'abc');");
    assert!(!opcodes.contains(&OpCode::DirectInternalCall1));
    assert!(!opcodes.contains(&OpCode::Strlen));
    assert!(!opcodes.contains(&OpCode::Strlen_Cv));
    assert!(opcodes.contains(&OpCode::InitFcall));
    assert!(opcodes.contains(&OpCode::SendNamed));
    assert!(opcodes.contains(&OpCode::DoFcall));

    let binary = main_opcodes("<?php intdiv(dividend: 9, divisor: 2);");
    assert!(!binary.contains(&OpCode::DirectInternalCall2));
    assert!(binary.contains(&OpCode::InitFcall));
    assert!(binary.contains(&OpCode::SendNamed));
    assert!(binary.contains(&OpCode::DoFcall));
}

#[test]
fn test_direct_builtin_lowering_preserves_namespace_resolution() {
    let out = run_php(r#"<?php
namespace DirectBuiltinShadow {
    function strlen($value) { return 99; }
    echo strlen('abc');
    echo ':';
    echo \strlen('abc');
}
"#);
    assert_eq!(out, "99:3");

    let shadowed = main_opcodes(r#"<?php
namespace DirectBuiltinShadow {
    function strlen($value) { return 99; }
    strlen('abc');
}
"#);
    assert!(!shadowed.contains(&OpCode::DirectInternalCall1));
    assert!(!shadowed.contains(&OpCode::Strlen));
    assert!(!shadowed.contains(&OpCode::Strlen_Cv));

    let global = main_opcodes(r#"<?php
namespace DirectBuiltinGlobal {
    \strlen('abc');
}
"#);
    assert!(global.contains(&OpCode::Strlen));
}

// -- call_user_func_array callback and argument shapes --

#[test]
fn test_call_user_func_array_function_packed_args() {
    let out = run_php(r#"<?php
function add_array_args($a, $b) { return $a + $b; }
echo call_user_func_array('add_array_args', [3, 7]);
"#);
    assert_eq!(out, "10");
}

#[test]
fn test_direct_internal_callback_matches_hash_frame_fallback() {
    let out = run_php(r#"<?php
$packed_args = ['abcd', 2, '|'];
$packed = call_user_func_array('chunk_split', $packed_args);
$hash = call_user_func_array('chunk_split', [4 => 'abcd', 8 => 2, 12 => '|']);
echo $packed . ':' . $hash;
"#);
    assert_eq!(out, "ab|cd|:ab|cd|");
}

#[test]
fn test_direct_internal_callback_preserves_scalar_coercion() {
    let out = run_php(r#"<?php
$strlen_args = [12345];
echo call_user_func_array('strlen', $strlen_args);
echo ':';
$abs_args = [-7];
echo call_user_func_array('abs', $abs_args);
"#);
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
    assert!(literal.contains(&OpCode::SendUser));
    assert!(!literal.contains(&OpCode::CallUserFuncArray));
}

#[test]
fn test_one_arg_callback_site_mixes_direct_and_framed_results() {
    let out = run_php(r#"<?php
function callback_passthrough($value) { return '<' . $value . '>'; }
$callbacks = ['strtoupper', 'callback_passthrough', 'strlen', 'strtolower'];
foreach ($callbacks as $callback) {
    echo call_user_func_array($callback, ['AbC']);
    echo ':';
}
echo call_user_func('ord', 'Z');
"#);
    assert_eq!(out, "ABC:<AbC>:3:abc:90");
}

#[test]
fn test_lowered_callback_calls_preserve_argument_evaluation_order() {
    let out = run_php(r#"<?php
function make_callback() { echo 'C'; return 'strlen'; }
function make_argument() { echo 'A'; return 'abc'; }
echo call_user_func(make_callback(), make_argument());
echo ':';
echo call_user_func_array(make_callback(), [make_argument()]);
"#);
    assert_eq!(out, "CA3:CA3");
}

#[test]
fn test_namespaced_shadow_keeps_normal_function_call() {
    let out = run_php(r#"<?php
namespace CallbackShadow {
    function call_user_func_array($callback, $args) { return 'shadow'; }
    echo call_user_func_array('strlen', ['abc']);
}
"#);
    assert_eq!(out, "shadow");
}

#[test]
fn test_lowered_call_user_func_keeps_by_value_semantics() {
    let out = run_php(r#"<?php
function bump_callback(&$value) { $value = $value + 1; }
$value = 1;
call_user_func('bump_callback', $value);
echo $value;
"#);
    assert_eq!(out, "1");
}

#[test]
fn test_call_user_func_array_closure_with_capture() {
    let out = run_php(r#"<?php
$factor = 4;
$multiply = function($value) use ($factor) { return $value * $factor; };
echo call_user_func_array($multiply, [6]);
"#);
    assert_eq!(out, "24");
}

#[test]
fn test_call_user_func_array_instance_and_static_methods() {
    let out = run_php(r#"<?php
class CallbackMath {
    public function add($a, $b) { return $a + $b; }
    public static function multiply($a, $b) { return $a * $b; }
}
$math = new CallbackMath();
echo call_user_func_array([$math, 'add'], [2, 5]);
echo ':';
echo call_user_func_array(['CallbackMath', 'multiply'], [3, 4]);
"#);
    assert_eq!(out, "7:12");
}

#[test]
fn test_call_user_func_array_invokable_object() {
    let out = run_php(r#"<?php
class InvokableCallback {
    public function __invoke($value) { return $value * 5; }
}
$callback = new InvokableCallback();
echo call_user_func_array($callback, [3]);
"#);
    assert_eq!(out, "15");
}

#[test]
fn test_call_user_func_array_trait_method() {
    let out = run_php(r#"<?php
trait CallbackTrait {
    public function triple($value) { return $value * 3; }
}
class TraitCallback {
    use CallbackTrait;
}
$callback = new TraitCallback();
echo call_user_func_array([$callback, 'triple'], [4]);
"#);
    assert_eq!(out, "12");
}

#[test]
fn test_call_user_func_array_named_method_args() {
    let out = run_php(r#"<?php
class NamedCallback {
    public function format($left, $right) { return $left . ':' . $right; }
}
$callback = new NamedCallback();
echo call_user_func_array([$callback, 'format'], ['right' => 'R', 'left' => 'L']);
"#);
    assert_eq!(out, "L:R");
}

#[test]
fn test_call_user_func_array_sparse_integer_keys_stay_positional() {
    let out = run_php(r#"<?php
function sparse_args($first, $second) { return $first . ':' . $second; }
echo call_user_func_array('sparse_args', [7 => 'A', 11 => 'B']);
"#);
    assert_eq!(out, "A:B");
}

#[test]
fn test_call_user_func_array_exception_cleans_callback_frame() {
    let out = run_php(r#"<?php
function callback_boom($value) { throw new Exception('boom'); }
function callback_ok($value) { return $value + 1; }
try {
    call_user_func_array('callback_boom', [1]);
} catch (Throwable $e) {
    echo 'caught:';
}
echo call_user_func_array('callback_ok', [4]);
"#);
    assert_eq!(out, "caught:5");
}

#[test]
fn test_call_user_func_array_cache_tracks_mutated_callback_name() {
    let out = run_php(r#"<?php
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
"#);
    assert_eq!(out, "a1:ab2:a3");
}

#[test]
fn test_call_user_func_array_same_closure_body_uses_current_captures() {
    let out = run_php(r#"<?php
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
"#);
    assert_eq!(out, "7:15");
}

#[test]
fn test_array_walk_method_reads_back_first_public_argument() {
    let out = run_php(r#"<?php
class Walker {
    public function bump(&$value, $key) { $value += 10; }
}
$walker = new Walker();
$values = [1, 2];
array_walk($values, [$walker, 'bump']);
echo implode(',', $values);
"#);
    assert_eq!(out, "11,12");
}

// -- is_callable with string --

#[test]
fn test_is_callable_existing_function() {
    let out = run_php(r#"<?php
echo is_callable('strtoupper') ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_nonexistent_function() {
    let out = run_php(r#"<?php
echo is_callable('nonexistent_func') ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_user_function() {
    let out = run_php(r#"<?php
function myFunc() {}
echo is_callable('myFunc') ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

// -- is_callable with non-callable values --

#[test]
fn test_is_callable_integer() {
    let out = run_php(r#"<?php
echo is_callable(42) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_null() {
    let out = run_php(r#"<?php
echo is_callable(null) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

// -- call_user_func with closure --

#[test]
fn test_call_user_func_closure() {
    let out = run_php(r#"<?php
$fn = function($x) { return $x * 3; };
echo call_user_func($fn, 4);
"#);
    assert_eq!(out, "12");
}

#[test]
fn test_lowered_closure_call_keeps_optional_gap_before_capture() {
    let out = run_php(r#"<?php
$prefix = 'P';
$format = function($value, $suffix = '!') use ($prefix) {
    return $prefix . $value . $suffix;
};
echo call_user_func($format, 'x');
echo ':';
echo call_user_func_array($format, ['y']);
"#);
    assert_eq!(out, "Px!:Py!");
}

#[test]
fn test_is_callable_closure() {
    let out = run_php(r#"<?php
$fn = function() { return 1; };
echo is_callable($fn) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

// -- call_user_func / is_callable with array callback --

#[test]
fn test_call_user_func_array_method() {
    let out = run_php(r#"<?php
class Greeter {
    public function greet($name) {
        return "Hello, " . $name;
    }
}
$g = new Greeter();
echo call_user_func([$g, 'greet'], 'World');
"#);
    assert_eq!(out, "Hello, World");
}

#[test]
fn test_is_callable_array_method() {
    let out = run_php(r#"<?php
class Foo {
    public function bar() {}
}
$f = new Foo();
echo is_callable([$f, 'bar']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_array_nonexistent_method() {
    let out = run_php(r#"<?php
class Foo {
    public function bar() {}
}
$f = new Foo();
echo is_callable([$f, 'nonexistent']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

// -- call_user_func / is_callable with static method ["ClassName", "method"] --

#[test]
fn test_call_user_func_static_method() {
    let out = run_php(r#"<?php
class Math {
    public static function double($x) {
        return $x * 2;
    }
}
echo call_user_func(['Math', 'double'], 5);
"#);
    assert_eq!(out, "10");
}

#[test]
fn test_is_callable_static_method() {
    let out = run_php(r#"<?php
class Util {
    public static function helper() { return 1; }
}
echo is_callable(['Util', 'helper']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_static_nonexistent_method() {
    let out = run_php(r#"<?php
class Util {
    public static function helper() { return 1; }
}
echo is_callable(['Util', 'nope']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

// -- visibility enforcement --

#[test]
fn test_is_callable_private_method_from_outside() {
    let out = run_php(r#"<?php
class Secret {
    private function hidden() { return 42; }
}
$s = new Secret();
echo is_callable([$s, 'hidden']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_protected_method_from_outside() {
    let out = run_php(r#"<?php
class Base {
    protected function internal() { return 1; }
}
$b = new Base();
echo is_callable([$b, 'internal']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_non_static_via_string_class() {
    // ["ClassName", "instanceMethod"] should be false — method is not static
    let out = run_php(r#"<?php
class Foo {
    public function bar() { return 1; }
}
echo is_callable(['Foo', 'bar']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_call_user_func_private_method_throws() {
    let out = run_php(r#"<?php
class Secret {
    private function hidden() { return 42; }
}
$s = new Secret();
try {
    call_user_func([$s, 'hidden']);
    echo "no_error";
} catch (\Throwable $e) {
    echo "caught";
}
"#);
    assert_eq!(out, "caught");
}

// -- inherited method callbacks --

#[test]
fn test_is_callable_inherited_instance_method() {
    let out = run_php(r#"<?php
class A {
    public function foo() { return 42; }
}
class B extends A {}
$b = new B();
echo is_callable([$b, 'foo']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_call_user_func_inherited_instance_method() {
    let out = run_php(r#"<?php
class A {
    public function foo() { return 42; }
}
class B extends A {}
$b = new B();
echo call_user_func([$b, 'foo']);
"#);
    assert_eq!(out, "42");
}

#[test]
fn test_is_callable_inherited_static_method() {
    let out = run_php(r#"<?php
class A {
    public static function bar() { return 99; }
}
class B extends A {}
echo is_callable(['B', 'bar']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_call_user_func_inherited_static_method() {
    let out = run_php(r#"<?php
class A {
    public static function bar() { return 99; }
}
class B extends A {}
echo call_user_func(['B', 'bar']);
"#);
    assert_eq!(out, "99");
}
