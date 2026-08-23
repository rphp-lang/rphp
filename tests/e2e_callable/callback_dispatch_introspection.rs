#[test]
fn known_by_value_callback_skips_only_the_reference_warning_probe() {
    let compiled = compile_source(
        "<?php function by_value($value) {} function by_ref(&$value) {} call_user_func('by_value', 1); call_user_func('by_ref', 1);",
    );
    let sends = compiled
        .main
        .instructions
        .iter()
        .filter(|instruction| {
            matches!(
                instruction.opcode,
                OpCode::SendUser | OpCode::SendUserChecked
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(sends.len(), 2);
    assert_eq!(sends[0].opcode, OpCode::SendUser);
    assert_eq!(sends[1].opcode, OpCode::SendUserChecked);
}

#[test]
fn call_user_func_forwards_named_and_variadic_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
function callback_named_target($a = 'a', $b = 'b', $c = 'c') {
    echo "$a,$b,$c\n";
}
function callback_variadic_target(...$args) { var_dump($args); }
function callback_required_target($a, $b) {}
function callback_reference_target(&$ref) {}
class CallbackBindingProbe {}
$closure = function($a = 'a', $b = 'b', $c = 'c') {
    echo "$a,$b,$c\n";
};
call_user_func('callback_named_target', 'A', c: 'C');
call_user_func('callback_named_target', c: 'C', a: 'A');
call_user_func('callback_variadic_target', 'A', c: 'C');
set_error_handler(function($_severity, $message) { echo "warning:$message\n"; });
call_user_func('callback_reference_target', ref: null);
restore_error_handler();
try { call_user_func('callback_required_target', b: 'B'); }
catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
var_dump(call_user_func('call_user_func', 'callback_named_target', c: 'D'));
$closure->__invoke('I', c: 'C');
$closure->call(new CallbackBindingProbe, 'B', c: 'C');
"#,
        ),
        concat!(
            "A,b,C\n",
            "A,b,C\n",
            "array(2) {\n",
            "  [0]=>\n",
            "  string(1) \"A\"\n",
            "  [\"c\"]=>\n",
            "  string(1) \"C\"\n",
            "}\n",
            "warning:callback_reference_target(): Argument #1 ($ref) must be passed by reference, value given\n",
            "ArgumentCountError:callback_required_target(): Argument #1 ($a) not passed\n",
            "a,b,D\n",
            "NULL\n",
            "I,b,C\n",
            "B,b,C\n",
        ),
    );
}

#[test]
fn is_callable_reports_syntax_and_canonical_names() {
    assert_eq!(
        run_php(
            r#"<?php
class CallableIntrospectionProbe {
    public function open() {}
    private function hidden() {}
    public function __invoke() {}
}
$object = new CallableIntrospectionProbe;
$closure = function() {};
foreach ([
    ['strlen', false],
    ['missing', true],
    [[$object, 'open'], false],
    [[$object, 'hidden'], true],
    [[$closure, '__invoke'], true],
    [$object, false],
    [new stdClass, false],
    [null, true],
    [[], true],
] as [$value, $syntax]) {
    $name = 'seed';
    var_dump(is_callable($value, $syntax, $name));
    echo json_encode($name), "\n";
}
"#,
        ),
        concat!(
            "bool(true)\n\"strlen\"\n",
            "bool(true)\n\"missing\"\n",
            "bool(true)\n\"CallableIntrospectionProbe::open\"\n",
            "bool(true)\n\"CallableIntrospectionProbe::hidden\"\n",
            "bool(true)\n\"Closure::__invoke\"\n",
            "bool(true)\n\"CallableIntrospectionProbe::__invoke\"\n",
            "bool(false)\n\"stdClass::__invoke\"\n",
            "bool(false)\n\"\"\n",
            "bool(false)\n\"Array\"\n",
        ),
    );
}

#[test]
fn call_user_func_autoloads_callback_classes_and_names_visibility_errors() {
    assert_eq!(
        run_php(
            r#"<?php
spl_autoload_register(function($class) {
    echo "autoload:$class\n";
    if ($class === 'CallbackLateLoaded') {
        class CallbackLateLoaded {
            public static function run($value) { echo "run:$value\n"; }
        }
    }
});
call_user_func([CallbackLateLoaded::class, 'run'], 'ready');
class CallbackVisibilityParent { protected function hidden() {} }
class CallbackVisibilityChild extends CallbackVisibilityParent {}
try { call_user_func([new CallbackVisibilityChild, 'hidden']); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "autoload:CallbackLateLoaded\n",
            "run:ready\n",
            "call_user_func(): Argument #1 ($callback) must be a valid callback, cannot access protected method CallbackVisibilityChild::hidden()\n",
        ),
    );
}

#[test]
fn call_user_func_array_preserves_only_explicit_reference_elements() {
    assert_eq!(
        run_php(
            r#"<?php
function callback_reference_mutator(&$value) { $value = 'changed'; }
set_error_handler(function($_severity, $message) { echo "warning:$message\n"; });
$plain = ['original'];
call_user_func_array('callback_reference_mutator', $plain);
echo $plain[0], "\n";
$referenced = ['original'];
$alias =& $referenced[0];
call_user_func_array('callback_reference_mutator', $referenced);
echo $referenced[0], "\n";
restore_error_handler();
"#,
        ),
        concat!(
            "warning:callback_reference_mutator(): Argument #1 ($value) must be passed by reference, value given\n",
            "original\n",
            "changed\n",
        ),
    );
}

#[test]
fn forward_static_call_preserves_compatible_called_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class ForwardRoot {
    public static function inspect(...$args) {
        echo static::class, ':', json_encode($args), "\n";
    }
    public function instance() {}
}
class ForwardChild extends ForwardRoot {
    public static function run() {
        forward_static_call([ForwardRoot::class, 'inspect'], 'P');
        forward_static_call_array(
            [ForwardRoot::class, 'inspect'],
            ['Q', 'named' => 'N'],
        );
        try { forward_static_call([ForwardRoot::class, 'instance']); }
        catch (Throwable $error) {
            echo get_class($error), ':', $error->getMessage(), "\n";
        }
    }
}
class ForwardLeaf extends ForwardChild {}
ForwardLeaf::run();
try { forward_static_call([ForwardRoot::class, 'inspect']); }
catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "ForwardLeaf:[\"P\"]\n",
            "ForwardLeaf:{\"0\":\"Q\",\"named\":\"N\"}\n",
            "TypeError:forward_static_call(): Argument #1 ($callback) must be a valid callback, non-static method ForwardRoot::instance() cannot be called statically\n",
            "Error:Cannot call forward_static_call() when no class scope is active\n",
        ),
    );
}

#[test]
fn callback_function_signatures_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'call_user_func',
    'call_user_func_array',
    'forward_static_call',
    'forward_static_call_array',
    'is_callable',
] as $function) {
    $reflection = new ReflectionFunction($function);
    echo $function, ':', $reflection->getNumberOfParameters(), '/',
        $reflection->getNumberOfRequiredParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(),
            $parameter->isVariadic() ? '...' : '',
            $parameter->isPassedByReference() ? '&' : '', ';';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "call_user_func:2/1:callback;args...;\n",
            "call_user_func_array:2/2:callback;args;\n",
            "forward_static_call:2/1:callback;args...;\n",
            "forward_static_call_array:2/2:callback;args;\n",
            "is_callable:3/1:value;syntax_only;callable_name&;\n",
        ),
    );
}
