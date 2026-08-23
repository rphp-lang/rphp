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
fn closure_values_have_regular_object_class_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$closure = static function () {};
echo get_class($closure), ':';
echo method_exists($closure, '__invoke') ? 'method:' : 'missing:';
echo method_exists($closure, '__INVOKE') ? 'case:' : 'missing:';
echo is_a($closure, 'closure') ? 'is-a:' : 'not-a:';
echo is_subclass_of($closure, Closure::class) ? 'subclass:' : 'same:';
echo count(class_implements($closure)), ':';
echo count(class_parents($closure)), ':';
echo count(class_uses($closure));
"#,
        ),
        "Closure:method:case:is-a:same:0:0:0"
    );
}

#[test]
fn closure_from_callable_preserves_callable_shape_scope_and_identity() {
    assert_eq!(
        run_php(
            r#"<?php
function add_pair(int $left, int $right): int { return $left + $right; }
class CallableFactory {
    private int $value;
    public function __construct(int $value) { $this->value = $value; }
    public function read(): int { return $this->value; }
    public static function twice(int $value): int { return $value * 2; }
    public static function __callStatic(string $name, array $args): string {
        return $name . ':' . implode(',', $args);
    }
}
class NonStaticOnly { public function read(): void {} }

$function = Closure::fromCallable('add_pair');
echo $function(2, 3), ':';
$boundFunction = $function->bindTo(new stdClass);
echo $boundFunction(4, 5), ':';

$object = new CallableFactory(7);
$method = Closure::fromCallable([$object, 'read']);
echo $method(), ':', $method->call(new CallableFactory(9)), ':';
echo Closure::fromCallable([CallableFactory::class, 'twice'])(6), ':';
echo Closure::fromCallable([CallableFactory::class, 'missing'])('x'), ':';

$existing = fn (): string => 'same';
echo Closure::fromCallable($existing) === $existing ? 'identity:' : 'copy:';
try { Closure::fromCallable([NonStaticOnly::class, 'read']); }
catch (TypeError $error) { echo $error->getMessage(); }
"#,
        ),
        "5:9:7:9:12:missing:x:identity:Failed to create closure from callable: non-static method NonStaticOnly::read() cannot be called statically"
    );
}

#[test]
fn closure_invoke_array_is_a_regular_callback_and_preserves_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$suffix = '!';
$closure = function (string $value) use ($suffix): string { return $value . $suffix; };
$callback = [$closure, '__INVOKE'];
echo $callback('direct'), ':';
echo call_user_func($callback, 'helper'), ':';
echo is_callable($callback) ? 'callable:' : 'missing:';
$reflected = Closure::fromCallable($callback);
echo $reflected === $closure ? 'same:' : 'copy:';
try { [$closure, 'missing'](); }
catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        "direct!:helper!:callable:same:Call to undefined method Closure::missing()"
    );
}

#[test]
fn closure_explicit_invoke_forwards_positional_named_and_dynamic_calls() {
    assert_eq!(
        run_php(
            r#"<?php
$suffix = '!';
$closure = function (string $value, string $tail = '?') use ($suffix): string {
    return $value . $suffix . $tail;
};
echo $closure->__invoke('direct', '.'), '|';
$method = '__INVOKE';
echo $closure->{$method}(value: 'named'), '|';

class ExplicitInvokeReceiver {
    private string $value = 'method';
    public function callback(): Closure { return $this->read(...); }
    private function read(string $tail): string { return $this->value . $tail; }
}
echo (new ExplicitInvokeReceiver())->callback()->__invoke('!');
$references = function (&$first, &...$rest) { $first++; $rest[0]++; };
$first = 1;
$second = 2;
$references->__INVOKE($first, $second);
echo '|', $first, ':', $second, '|';
try { $references->__invoke(null, $second); }
catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        "direct!.|named!?|method!|2:3|Closure::__invoke(): Argument #1 ($first) could not be passed by reference"
    );
}

#[test]
fn closure_var_dump_reports_function_receiver_captures_and_parameters() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function reflectedDebug(string $required, int $optional = 1) {}
var_dump(Closure::fromCallable('reflectedDebug'));

class DebugReceiver {
    public function method() {}
    public function closure(): Closure {
        $captured = 'kept';
        return function ($argument) use ($captured) {};
    }
}
$receiver = new DebugReceiver();
var_dump(Closure::fromCallable([$receiver, 'method']));
var_dump($receiver->closure());
"#,
            "closure-debug.php",
            ".",
        ),
        r#"object(Closure)#1 (2) {
  ["function"]=>
  string(14) "reflectedDebug"
  ["parameter"]=>
  array(2) {
    ["$required"]=>
    string(10) "<required>"
    ["$optional"]=>
    string(10) "<optional>"
  }
}
object(Closure)#2 (2) {
  ["function"]=>
  string(21) "DebugReceiver::method"
  ["this"]=>
  object(DebugReceiver)#1 (0) {
  }
}
object(Closure)#2 (6) {
  ["name"]=>
  string(36) "{closure:DebugReceiver::closure():9}"
  ["file"]=>
  string(17) "closure-debug.php"
  ["line"]=>
  int(9)
  ["static"]=>
  array(1) {
    ["captured"]=>
    string(4) "kept"
  }
  ["this"]=>
  object(DebugReceiver)#1 (0) {
  }
  ["parameter"]=>
  array(1) {
    ["$argument"]=>
    string(10) "<required>"
  }
}
"#
    );
}

#[test]
fn closure_var_dump_reports_known_static_defaults_before_first_call() {
    let output = run_php_with_source_context(
        r#"<?php
$closure = function () {
    static $known = [];
    static $dynamic = strlen('ab');
    $known[] = ++$dynamic;
};
var_dump($closure);
$closure();
var_dump($closure);
"#,
        "closure-static-debug.php",
        ".",
    );
    assert!(output.contains("[\"known\"]=>\n    array(0)"));
    assert!(output.contains("[\"dynamic\"]=>\n    NULL"));
    assert!(output.contains("[\"known\"]=>\n    array(1)"));
    assert!(output.ends_with("[\"dynamic\"]=>\n    int(3)\n  }\n}\n"));
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
fn closure_call_temporarily_rebinds_receiver_scope_and_runtime_cache() {
    assert_eq!(
        run_php(
            r#"<?php
class CallBox {
    private int $value;
    private static int $visible = 1;
    public function __construct(int $value) { $this->value = $value; }
    public function multiplier(): Closure {
        return function (int $factor): int { return $this->value * $factor; };
    }
    public static function scopeProbe(): Closure {
        return function (): bool { return isset(CallBox::$visible); };
    }
}

$original = new CallBox(2);
$replacement = new CallBox(7);
$multiply = $original->multiplier();
echo $multiply(3), ':', $multiply->call($replacement, 4), ':', $multiply(5), '|';

$probe = CallBox::scopeProbe();
echo $probe() ? 'T' : 'F';
echo $probe->call(new class {}) ? 'T' : 'F';
echo $probe() ? 'T' : 'F';
try { $multiply->call(null); } catch (TypeError $error) { echo '|type'; }
"#,
        ),
        "6:28:10|TFT|type"
    );
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

#[test]
fn invalid_dynamic_callable_values_throw_catchable_errors() {
    let out = run_php(
        r#"<?php
$values = [null, 'missing_dynamic_function', ['only-one'], new stdClass];
foreach ($values as $value) {
    try {
        $value();
    } catch (Error $error) {
        echo $error->getMessage(), "\n";
    }
}
echo "continued";
"#,
    );
    assert_eq!(
        out,
        "Value of type null is not callable\n\
Call to undefined function missing_dynamic_function()\n\
Array callback must have exactly two elements\n\
Object of type stdClass is not callable\n\
continued"
    );
}

#[test]
fn invalid_dynamic_call_preserves_source_line_and_validation_order() {
    let source = "<?php\ntry {\n    $class = null;\n    $class::$undefinedMethod();\n} catch (Error $error) {\n    echo $error->getMessage();\n}\n";
    let compiled = compile_source(source);
    let init_index = compiled
        .main
        .instructions
        .iter()
        .position(|instruction| instruction.opcode == OpCode::InitDynamicCall)
        .unwrap();

    assert_eq!(compiled.main.source_line(init_index), Some(4));
    assert_eq!(
        run_php(source),
        "Class name must be a valid object or a string"
    );
}

#[test]
fn non_static_class_callbacks_require_a_compatible_receiver() {
    let out = run_php(
        r#"<?php
class ReceiverBase {
    public function method() { echo get_class($this), "\n"; }
    public function compatible() { ReceiverBase::method(); self::method(); }
}
class ReceiverChild extends ReceiverBase {
    public function inherited() { ReceiverBase::method(); parent::method(); }
}
class IncompatibleReceiver {
    public function call() { ReceiverBase::method(); }
}
class MagicInstanceOnly {
    public function __call($name, $arguments) {}
}

(new ReceiverBase)->compatible();
(new ReceiverChild)->inherited();

$callbacks = [
    ['ReceiverBase', 'method'],
    'ReceiverBase::method',
    ['MagicInstanceOnly', 'missing'],
    'MagicInstanceOnly::missing',
];
foreach ($callbacks as $callback) {
    try { $callback(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
try { (new IncompatibleReceiver)->call(); }
catch (Error $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(
        out,
        "ReceiverBase\nReceiverBase\nReceiverChild\nReceiverChild\n\
Non-static method ReceiverBase::method() cannot be called statically\n\
Non-static method ReceiverBase::method() cannot be called statically\n\
Non-static method MagicInstanceOnly::missing() cannot be called statically\n\
Non-static method MagicInstanceOnly::missing() cannot be called statically\n\
Non-static method ReceiverBase::method() cannot be called statically"
    );
}

#[test]
fn first_class_callable_creation_preserves_the_resolution_error() {
    let out = run_php(
        r#"<?php
class CallableDiagnosticTarget {
    private static function hidden() {}
    protected static function guarded() {}
    public function instance() {}

    public static function inside() {
        self::hidden(...);
        self::guarded(...);
        echo "inside\n";
    }
}

function report($factory) {
    try {
        $factory();
    } catch (Error $error) {
        echo get_class($error), ': ', $error->getMessage(), "\n";
    }
}

report(fn() => (42)(...));
report(fn() => missing_callable_function(...));
report(fn() => MissingCallableClass::method(...));
report(fn() => CallableDiagnosticTarget::missing(...));
report(fn() => (new CallableDiagnosticTarget)->missing(...));
report(fn() => CallableDiagnosticTarget::hidden(...));
report(fn() => CallableDiagnosticTarget::guarded(...));
report(fn() => CallableDiagnosticTarget::instance(...));
CallableDiagnosticTarget::inside();
?>"#,
    );
    assert_eq!(
        out,
        "Error: Value of type int is not callable\n\
Error: Call to undefined function missing_callable_function()\n\
Error: Class \"MissingCallableClass\" not found\n\
Error: Call to undefined method CallableDiagnosticTarget::missing()\n\
Error: Call to undefined method CallableDiagnosticTarget::missing()\n\
Error: Call to private method CallableDiagnosticTarget::hidden() from global scope\n\
Error: Call to protected method CallableDiagnosticTarget::guarded() from global scope\n\
Error: Non-static method CallableDiagnosticTarget::instance() cannot be called statically\n\
inside\n"
    );
}

#[test]
fn named_first_class_callable_errors_keep_the_creation_origin_and_trace() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nnamespace FccOrigin;\nfunction capture() {\n    try {\n        missing(...);\n    } catch (\\Throwable $error) {\n        echo $error->getMessage(), '|', $error->getFile(), ':', $error->getLine(), \"\\n\";\n        echo $error->getTraceAsString();\n    }\n}\ncapture();",
            "/fixture/fcc-origin.php",
            "/fixture",
        ),
        "Call to undefined function FccOrigin\\missing()|/fixture/fcc-origin.php:5\n#0 /fixture/fcc-origin.php(11): FccOrigin\\capture()\n#1 {main}"
    );
}

#[test]
fn dynamic_first_class_callables_execute_and_keep_resolution_origins() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nnamespace DynamicFcc;\nclass Target {\n    public static function twice($value) { return $value * 2; }\n    public function plus($value) { return $value + 1; }\n}\nfunction local($value) { return $value . '!'; }\n$class = Target::class;\n$staticMethod = 'twice';\n$static = $class::$staticMethod(...);\n$target = new Target;\n$instanceMethod = 'plus';\n$instance = $target->$instanceMethod(...);\n$local = namespace\\local(...);\necho $static(4), '|', $instance(4), '|', $local('ok'), \"\\n\";\nfunction capture() {\n    $class = Target::class;\n    $missing = 'missing';\n    try {\n        $class::$missing(...);\n    } catch (\\Throwable $error) {\n        echo $error->getMessage(), '|', $error->getFile(), ':', $error->getLine(), \"\\n\";\n        echo $error->getTraceAsString();\n    }\n}\ncapture();",
            "/fixture/dynamic-fcc.php",
            "/fixture",
        ),
        "8|5|ok!\nCall to undefined method DynamicFcc\\Target::missing()|/fixture/dynamic-fcc.php:20\n#0 /fixture/dynamic-fcc.php(26): DynamicFcc\\capture()\n#1 {main}"
    );
}

#[test]
fn dynamic_first_class_callable_evaluates_owner_and_member_once_in_order() {
    assert_eq!(
        run_php(
            "<?php class OrderedFcc { public static function twice($value) { echo 'call>'; return $value * 2; } public function plus($value) { echo 'invoke>'; return $value + 1; } } function fccOwner() { echo 'owner>'; return OrderedFcc::class; } function fccReceiver() { echo 'receiver>'; return new OrderedFcc; } function staticFccName() { echo 'static-name>'; return 'twice'; } function instanceFccName() { echo 'instance-name>'; return 'plus'; } $static = (fccOwner())::{staticFccName()}(...); echo $static(3), '|'; $instance = fccReceiver()->{instanceFccName()}(...); echo $instance(3);"
        ),
        "owner>static-name>call>6|receiver>instance-name>invoke>4"
    );
}

#[test]
fn first_class_callable_keeps_existing_closure_identity() {
    let out = run_php(
        r#"<?php
$closure = function() { return 'same'; };
$direct = $closure(...);
$invoke = $closure->__invoke(...);
var_dump($direct === $closure, $invoke === $closure, $direct === $invoke);
echo $invoke();
?>"#,
    );
    assert_eq!(
        out,
        "bool(true)\nbool(true)\nbool(true)\nsame"
    );
}

#[test]
fn first_class_callable_dispatches_magic_instance_and_static_methods() {
    let out = run_php(
        r#"<?php
class MagicCallableBase {
    public function __call($name, $arguments) {
        return 'instance:' . $name . ':' . implode(',', $arguments);
    }
    public static function __callStatic($name, $arguments) {
        return static::class . ':' . $name . ':' . implode(',', $arguments);
    }
}
class MagicCallableChild extends MagicCallableBase {}

$object = new MagicCallableBase;
$instance = $object->missing(...);
$array = [$object, 'other'];
$fromArray = $array(...);
$static = MagicCallableBase::unknown(...);
$inherited = MagicCallableChild::unknown(...);

echo $instance(1, 'x'), "\n";
echo $fromArray(), "\n";
echo $static('s'), "\n";
echo $inherited('i'), "\n";
echo $object->direct(2, 'd'), "\n";
echo MagicCallableChild::directStatic(5), "\n";
echo call_user_func([$object, 'callback'], 3), "\n";
echo call_user_func(['MagicCallableChild', 'staticCallback'], 4);
?>"#,
    );
    assert_eq!(
        out,
        "instance:missing:1,x\n\
instance:other:\n\
MagicCallableBase:unknown:s\n\
MagicCallableChild:unknown:i\n\
instance:direct:2,d\n\
MagicCallableChild:directStatic:5\n\
instance:callback:3\n\
MagicCallableChild:staticCallback:4"
    );
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
