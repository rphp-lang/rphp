// -- is_callable with string --

#[test]
fn internal_instance_method_first_class_callable_is_invokable() {
    assert_eq!(
        run_php(
            "<?php $invoke = (new ReflectionMethod(ReflectionMethod::class, 'getPrototype'))->invoke(...); $target = new ReflectionMethod(ReflectionMethod::class, 'getPrototype'); try { $invoke($target); } catch (ReflectionException $error) { echo 'caught'; }"
        ),
        "caught"
    );
}

#[test]
fn static_method_string_callbacks_preserve_the_first_public_argument() {
    assert_eq!(
        run_php(
            "<?php class StaticStringCallback { public static function invoke(string $value): void { echo $value; } } call_user_func('StaticStringCallback::invoke', 'direct:'); spl_autoload_register('StaticStringCallback::invoke'); class_exists('MissingStaticStringCallback');"
        ),
        "direct:MissingStaticStringCallback"
    );
}

#[test]
fn closure_values_are_instances_of_closure() {
    let out = run_php("<?php $closure = static function () {}; echo $closure instanceof Closure ? 'yes' : 'no';");
    assert_eq!(out, "yes");
}

#[test]
fn variadic_closure_arguments_do_not_overwrite_captures() {
    let out = run_php(
        "<?php $captured = 'kept'; $closure = static function (...$args) use ($captured) { return $captured . ':' . implode(',', $args); }; echo $closure('a', 'b', 'c');",
    );
    assert_eq!(out, "kept:a,b,c");
}

#[test]
fn closure_declared_in_method_retains_protected_visibility_scope() {
    let out = run_php(
        "<?php class ScopedParent { protected string $value = 'ok'; public function reader(object $target): Closure { return static fn () => $target->value; } } class ScopedChild extends ScopedParent {} $object = new ScopedChild(); echo $object->reader($object)();",
    );
    assert_eq!(out, "ok");
}

#[test]
fn dynamic_call_expands_a_sole_unpack_argument() {
    let out = run_php(
        "<?php $callable = static fn ($a, $b, $c) => $a . $b . $c; $args = ['a', 'b', 'c']; echo $callable(...$args);",
    );
    assert_eq!(out, "abc");
}

#[test]
fn ordinary_function_calls_expand_a_sole_unpack_argument_with_namespace_fallback() {
    let out = run_php(
        r#"<?php
namespace SpreadCompatibility {
    function join_values($left, $right) { return $left . ':' . $right; }

    $values = ['left', 'right'];
    echo join_values(...$values), '|';

    $groups = [
        ['same' => 1, 4 => 'a'],
        ['same' => 2, 9 => 'b'],
        ['tail' => 3],
    ];
    $merged = array_merge(...$groups);
    echo $merged['same'], ':', $merged[0], ':', $merged[1], ':', $merged['tail'];
}
"#,
    );
    assert_eq!(out, "left:right|2:a:b:3");
}

#[test]
fn ordinary_function_calls_flatten_mixed_positional_and_named_unpack_arguments() {
    let out = run_php(
        r#"<?php
function mixed_unpack_pair($left, $right) { return $left . ':' . $right; }
function mixed_unpack_triple($value, $left, $right) { return $value . ':' . $left . ':' . $right; }

$right = ['right' => 'R'];
echo mixed_unpack_pair('L', ...$right), '|';

$tail = ['x', 'y'];
echo mixed_unpack_triple(4, ...$tail), '|';

$methods = [['GET']];
echo array_merge([], ...$methods)[0];
"#,
    );
    assert_eq!(out, "L:R|4:x:y|GET");
}

#[test]
fn unpacked_calls_grow_the_vm_stack_for_large_argument_lists() {
    let out = run_php(
        r#"<?php
function first_value($first) { return $first; }
function delayed_first_value($first) { yield $first; }

$arguments = range(1, 20000);
echo first_value(...$arguments), '|';

$generator = delayed_first_value(...$arguments);
echo $generator->current(), '|';

$generator = call_user_func_array('delayed_first_value', $arguments);
echo $generator->current();
"#,
    );
    assert_eq!(out, "1|1|1");
}

#[test]
fn test_is_callable_existing_function() {
    let out = run_php(
        r#"<?php
echo is_callable('strtoupper') ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_nonexistent_function() {
    let out = run_php(
        r#"<?php
echo is_callable('nonexistent_func') ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_user_function() {
    let out = run_php(
        r#"<?php
function myFunc() {}
echo is_callable('myFunc') ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "true");
}

// -- is_callable with non-callable values --

#[test]
fn test_is_callable_integer() {
    let out = run_php(
        r#"<?php
echo is_callable(42) ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_null() {
    let out = run_php(
        r#"<?php
echo is_callable(null) ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "false");
}

// -- call_user_func with closure --

#[test]
fn test_call_user_func_closure() {
    let out = run_php(
        r#"<?php
$fn = function($x) { return $x * 3; };
echo call_user_func($fn, 4);
"#,
    );
    assert_eq!(out, "12");
}

#[test]
fn test_lowered_closure_call_keeps_optional_gap_before_capture() {
    let out = run_php(
        r#"<?php
$prefix = 'P';
$format = function($value, $suffix = '!') use ($prefix) {
    return $prefix . $value . $suffix;
};
echo call_user_func($format, 'x');
echo ':';
echo call_user_func_array($format, ['y']);
"#,
    );
    assert_eq!(out, "Px!:Py!");
}

#[test]
fn test_is_callable_closure() {
    let out = run_php(
        r#"<?php
$fn = function() { return 1; };
echo is_callable($fn) ? 'true' : 'false';
"#,
    );
    assert_eq!(out, "true");
}
