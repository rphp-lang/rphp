// -- call_user_func with string callback --

#[test]
fn test_call_user_func_strtoupper() {
    let out = run_php(
        r#"<?php
echo call_user_func('strtoupper', 'hello');
"#,
    );
    assert_eq!(out, "HELLO");
}

#[test]
fn test_call_user_func_strlen() {
    let out = run_php(
        r#"<?php
echo call_user_func('strlen', 'abc');
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_call_user_func_user_function() {
    let out = run_php(
        r#"<?php
function double($x) { return $x * 2; }
echo call_user_func('double', 5);
"#,
    );
    assert_eq!(out, "10");
}

#[test]
fn leading_namespace_separator_normalizes_string_function_callables() {
    let out = run_php(
        r#"<?php
namespace DynamicFqn;
function decorate($value) { return "D:$value"; }
class Decorator {
    public static function marker() { return 'S'; }
}
$slash = chr(92);
$callbacks = [
    $slash . 'strlen',
    $slash . 'DynamicFqn\\decorate',
    'DynamicFqn\\decorate',
];
foreach ($callbacks as $callback) {
    echo is_callable($callback) ? 'yes' : 'no', '|', $callback('abcd'), "\n";
}
$method = $slash . 'DynamicFqn\\Decorator::marker';
echo is_callable($method) ? 'yes' : 'no', '|', $method(), "\n";
echo call_user_func($slash . 'strlen', 'abc'), "\n";
echo \Closure::fromCallable($slash . 'strlen')('abcde'), "\n";
try {
    ('\literalMissing')();
} catch (\Throwable $error) {
    echo $error->getMessage(), "\n";
}
foreach ([$slash . 'missing', $slash . $slash . 'strlen', $slash] as $callback) {
    try {
        $callback();
    } catch (\Throwable $error) {
        echo $error->getMessage(), '|';
    }
    echo is_callable($callback) ? 'yes' : 'no', "\n";
}
"#,
    );
    assert_eq!(
        out,
        "yes|4\nyes|D:abcd\nyes|D:abcd\nyes|S\n3\n5\n\
Call to undefined function literalMissing()\n\
Call to undefined function \\missing()|no\n\
Call to undefined function \\\\strlen()|no\n\
Call to undefined function \\()|no\n"
    );
}

#[test]
fn test_call_user_func_multiple_args() {
    let out = run_php(
        r#"<?php
function add($a, $b) { return $a + $b; }
echo call_user_func('add', 3, 7);
"#,
    );
    assert_eq!(out, "10");
}

#[test]
fn test_call_user_func_is_lowered_without_wrapper_call() {
    let opcodes = main_opcodes("<?php call_user_func('strlen', 'abc');");
    assert!(opcodes.contains(&OpCode::InitUserCall));
    assert!(opcodes.contains(&OpCode::SendUserChecked));
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
    let out = run_php(
        r#"<?php
namespace DirectBinaryShadow {
    function intdiv($left, $right) { return 99; }
    echo intdiv(9, 2);
    echo ':';
    echo \intdiv(9, 2);
}
"#,
    );
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
    let compiled = compile_source(
        "<?php
function value() { return 7; }
for ($i = 0; $i < 3; $i++) { value(); }
$kept = value();
$old = $i++;
",
    );

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
    assert_eq!(
        scalar.functions[0].1.common.plan.cleanup,
        CleanupMode::SkipScan
    );

    let binary = compile_source(
        "<?php function binary_direct($left, $right) { return intdiv($left, $right); }",
    );
    assert_eq!(
        binary.functions[0].1.common.plan.cleanup,
        CleanupMode::SkipScan
    );

    let heap = compile_source("<?php function heap_direct($value) { return strtolower($value); }");
    assert_eq!(
        heap.functions[0].1.common.plan.cleanup,
        CleanupMode::ScanAll
    );
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
    let out = run_php(
        r#"<?php
namespace DirectBuiltinShadow {
    function strlen($value) { return 99; }
    echo strlen('abc');
    echo ':';
    echo \strlen('abc');
}
"#,
    );
    assert_eq!(out, "99:3");

    let shadowed = main_opcodes(
        r#"<?php
namespace DirectBuiltinShadow {
    function strlen($value) { return 99; }
    strlen('abc');
}
"#,
    );
    assert!(!shadowed.contains(&OpCode::DirectInternalCall1));
    assert!(!shadowed.contains(&OpCode::Strlen));
    assert!(!shadowed.contains(&OpCode::Strlen_Cv));

    let global = main_opcodes(
        r#"<?php
namespace DirectBuiltinGlobal {
    \strlen('abc');
}
"#,
    );
    assert!(global.contains(&OpCode::Strlen));
}
