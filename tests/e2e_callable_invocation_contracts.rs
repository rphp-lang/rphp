mod common;

use common::run_php;

#[test]
fn dynamic_static_strings_share_method_resolution_and_unpacking() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Target {
class Handler {
    public static function collect($a, $b, $c) { echo "$a,$b,$c\n"; }
    public static function __callStatic($name, $args) { var_dump($name, $args); }
}
}
namespace Caller {
$callback = 'Target\\Handler::collect';
$callback('a', 'b', 'c');
$args = ['d', 'e', 'f'];
$callback(...$args);
$callback = 'target\\handler::missing';
$callback(1, 2);
$callback = 'Target\\Handler::';
$callback();
}
"#,
        ),
        concat!(
            "a,b,c\n",
            "d,e,f\n",
            "string(7) \"missing\"\n",
            "array(2) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n}\n",
            "string(0) \"\"\n",
            "array(0) {\n}\n",
        ),
    );
}

#[test]
fn dynamic_array_errors_use_the_declared_class_spelling() {
    assert_eq!(
        run_php(
            r#"<?php
try { $callback = ['stdclass', 'missing']; $callback(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "Call to undefined method stdClass::missing()\n",
    );
}

#[test]
fn forward_static_array_validates_argument_order_before_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class ForwardMagic {
    public function __call($name, $arguments) {}
}
try { forward_static_call_array([new ForwardMagic, 'run'], ['named' => 1, 2]); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "Cannot use positional argument after named argument\n",
    );
}

#[test]
fn reflected_method_closures_preserve_binding_origin_rules() {
    assert_eq!(
        run_php(
            r#"<?php
class BindOwner { public function method() {} }
class BindChild extends BindOwner {}
class BindOther {}
set_error_handler(function($_severity, $message) { echo $message, "\n"; });
$method = (new ReflectionMethod(BindOwner::class, 'method'))->getClosure(new BindOwner);
var_dump($method->bindTo(null, BindOwner::class));
var_dump($method->bindTo(new BindOther, BindOwner::class));
var_dump($method->bindTo(new BindChild, BindOwner::class) instanceof Closure);
$internal = (new ReflectionMethod(SplStack::class, 'count'))->getClosure(new SplStack);
var_dump($internal->bindTo(new BindOther));
var_dump($internal->bindTo(new SplStack, BindOther::class));
var_dump($internal->bindTo(new SplStack, SplDoublyLinkedList::class) instanceof Closure);
"#,
        ),
        concat!(
            "Cannot unbind $this of method, this will be an error in PHP 9\nNULL\n",
            "Cannot bind method BindOwner::method() to object of class BindOther, this will be an error in PHP 9\nNULL\n",
            "bool(true)\n",
            "Cannot bind method SplDoublyLinkedList::count() to object of class BindOther, this will be an error in PHP 9\nNULL\n",
            "Cannot rebind scope of closure created from method, this will be an error in PHP 9\nNULL\n",
            "bool(true)\n",
        ),
    );
}

#[test]
fn closure_get_current_recurses_with_the_same_environment() {
    assert_eq!(
        run_php(
            r#"<?php
$count = 0;
$closure = function($depth) use (&$count) {
    $current = Closure::getCurrent();
    echo ++$count, ':', $depth, "\n";
    if ($depth < 3) { $current($depth + 1); }
};
$closure(1);
try { Closure::getCurrent(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
function ordinary() { return Closure::getCurrent(); }
try { ordinary(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "1:1\n2:2\n3:3\n",
            "Current function is not a closure\n",
            "Current function is not a closure\n",
        ),
    );
}

#[test]
fn explicit_closure_invoke_uses_the_method_argument_error_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class InvokeExpected {}
class InvokeActual {}
$closure = function(InvokeExpected $value) {};
try { $closure(new InvokeActual); }
catch (TypeError $error) { echo str_contains($error->getMessage(), ', called in ') ? "located\n" : "unlocated\n"; }
try { $closure->__invoke(new InvokeActual); }
catch (TypeError $error) { echo str_contains($error->getMessage(), ', called in ') ? "located\n" : "unlocated\n"; }
try { call_user_func($closure, new InvokeActual); }
catch (TypeError $error) { echo str_contains($error->getMessage(), ', called in ') ? "located\n" : "unlocated\n"; }
try { call_user_func([$closure, '__invoke'], new InvokeActual); }
catch (TypeError $error) { echo str_contains($error->getMessage(), ', called in ') ? "located\n" : "unlocated\n"; }
"#,
        ),
        "located\nunlocated\nlocated\nunlocated\n",
    );
}

#[test]
fn explicit_closure_invoke_preserves_references_and_mixed_variadic_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
class InvocationLayoutSentinel { public function __invoke() {} }
$byReference = function & (&$value) { return $value; };
$direct = 1;
$alias =& $byReference($direct);
$alias = 2;
$explicit = 3;
$alias =& $byReference->__invoke($explicit);
$alias = 4;
var_dump($direct, $explicit);

$variadic = function (...$arguments) { var_dump($arguments); };
$variadic->__invoke('A', c: 'C');
"#,
        ),
        concat!(
            "int(2)\n",
            "int(4)\n",
            "array(2) {\n",
            "  [0]=>\n",
            "  string(1) \"A\"\n",
            "  [\"c\"]=>\n",
            "  string(1) \"C\"\n",
            "}\n",
        ),
    );
}

#[test]
fn chained_dimension_receivers_release_unselected_temporary_objects() {
    assert_eq!(
        run_php(
            r#"<?php
class FirstParameter {}
class SecondParameter {}
function reflectedPair(FirstParameter $first, SecondParameter $second) {}
$function = new ReflectionFunction('reflectedPair');
error_reporting(0);
var_dump($function->getParameters()[0]->getClass());
"#,
        ),
        concat!(
            "object(ReflectionClass)#3 (1) {\n",
            "  [\"name\"]=>\n",
            "  string(14) \"FirstParameter\"\n",
            "}\n",
        ),
    );
}
