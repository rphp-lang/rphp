mod common;

use common::run_php;

macro_rules! oracle_case {
    ($name:ident, $source:literal, $expected:literal) => {
        #[test]
        fn $name() {
            assert_eq!(run_php($source), $expected);
        }
    };
}

oracle_case!(
    method_fast_and_closure_paths_preserve_unused_relative_type_contracts,
    r#"<?php
declare(strict_types=1);
class HintRoot {}
class HintOwner extends HintRoot {
    public $number = 9;
    function plain($value) { return $this->number + $value; }
    function wide(mixed $value) { return $this->number + $value; }
    function scoped($unused, mixed $wide, self $owner, parent $root, int $value) {
        return $this->number + $value;
    }
}
class HintChild extends HintOwner {}
function hintArgument($value) { echo 'arg:'; return $value; }
$owner = new HintChild;
$root = new HintRoot;
for ($i = 0; $i < 40; ++$i) {
    $a = $owner->plain(3);
    $b = $owner->wide(4);
    $c = $owner->scoped(null, [], $owner, $root, 5);
}
echo $a, ':', $b, ':', $c, "\n";
$number = 6;
$reference =& $number;
echo $owner->plain($reference), ':', $owner->wide($reference), "\n";
echo $owner->scoped(null, [], hintArgument($owner), $root, 7), "\n";
try { $owner->scoped(null, [], new stdClass, $root, 7); }
catch (TypeError $e) { echo "owner rejected\n"; }
try { $owner->scoped(null, [], $owner, hintArgument(new stdClass), 7); }
catch (TypeError $e) { echo "root rejected\n"; }
try { $owner->scoped(null, [], $owner, $root, '7'); }
catch (TypeError $e) { echo "strict rejected\n"; }
$bound = Closure::fromCallable([$owner, 'scoped']);
echo $bound(null, [], $owner, $root, 8), "\n";
try { $bound(null, [], new stdClass, $root, 8); }
catch (TypeError $e) { echo "closure rejected\n"; }
"#,
    r#"12:13:14
15:15
arg:16
owner rejected
arg:root rejected
strict rejected
17
closure rejected
"#
);

oracle_case!(
    weak_method_arguments_fall_back_before_scope_validation_and_body,
    r#"<?php
class WeakHintOwner {
    public $number = 9;
    function compute(self $unused, int $value) { return $this->number + $value; }
}
$owner = new WeakHintOwner;
for ($i = 0; $i < 40; ++$i) { $owner->compute($owner, 1); }
echo $owner->compute($owner, '2'), "\n";
$bound = Closure::fromCallable([$owner, 'compute']);
echo $bound($owner, '3'), "\n";
try { $owner->compute(new stdClass, '4'); }
catch (TypeError $e) { echo "scope rejected\n"; }
echo $owner->number, "\n";
"#,
    r#"11
12
scope rejected
9
"#
);

oracle_case!(
    named_declarations_cannot_impersonate_anonymous_origin_by_prefix,
    r#"<?php
function __closure_704($value = 7) { return $value; }
class PrefixOwner { function __closure_801($value = 9) { return $value; } }
$function = Closure::fromCallable('__closure_704');
$method = Closure::fromCallable([new PrefixOwner, '__closure_801']);
foreach ([$function, $method, function () { return 11; }] as $call) {
    echo (int)(new ReflectionFunction($call))->isAnonymous(), ':', $call(), "\n";
}
set_error_handler(function ($level, $message) { echo $message, "\n"; return true; });
var_dump($function->bindTo(null, PrefixOwner::class));
restore_error_handler();
echo $function(13), ':', $method(15), "\n";
"#,
    r#"0:7
0:9
1:11
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
13:15
"#
);

oracle_case!(
    named_function_binding_preserves_origin_without_rejecting_receiver_only,
    r#"<?php
class ReceiverOnly {}
function bindingOrigin($input) { return $input . '!'; }
set_error_handler(function ($level, $message) { echo $message, "\n"; return true; });
foreach ([Closure::fromCallable('bindingOrigin'), Closure::fromCallable('strlen')] as $f) {
    $object = new ReceiverOnly;
    $withReceiver = $f->bindTo($object, null);
    echo $withReceiver('abc'), ':', $f('abc'), "\n";
    $preserved = $f->bindTo(null, 'static');
    echo $preserved('a'), "\n";
    var_dump($f->bindTo(null, ReceiverOnly::class));
    var_dump($f->call($object, 'abc'));
}
restore_error_handler();
"#,
    r#"abc!:abc!
a!
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
3:3
1
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
"#
);

oracle_case!(
    parameter_keeps_anonymous_callable_alive,
    r#"<?php
$seed = 'kept';
$f = function ($suffix = '!') use ($seed) { static $calls = 0; return $seed . ++$calls . $suffix; };
$view = new ReflectionFunction($f);
$name = $view->getName();
$p = $view->getParameters()[0];
unset($view, $f, $seed);
$owner = $p->getDeclaringFunction();
echo get_class($owner), ':', (int)($owner->getName() === $name), "\n";
$again = $owner->getClosure();
unset($owner, $p);
echo $again(), ':', $again('?'), "\n";
"#,
    r#"ReflectionFunction:1
kept1!:kept2?
"#
);

oracle_case!(
    invoke_parameter_preserves_capture_and_reference_identity,
    r#"<?php
$total = 4;
$f = function (&$value, $delta = 2) use (&$total) { $value += $delta; return ++$total; };
$view = new ReflectionMethod($f, '__invoke');
$p = $view->getParameters()[0];
unset($view);
$method = $p->getDeclaringFunction();
echo get_class($method), ':', $method->getName(), ':', (int)$p->isPassedByReference(), "\n";
$call = $method->getClosure($f);
unset($p, $method, $f);
$value = 10;
echo $call($value), ':', $value, ':', $total, "\n";
"#,
    r#"ReflectionMethod:__invoke:1
5:12:5
"#
);

oracle_case!(
    object_reflection_uses_each_closure_signature,
    r#"<?php
foreach ([function ($x, $y = 3) {}, function ($x, $y, $z = null) {}] as $f) {
    $view = new ReflectionObject($f);
    $method = $view->getMethod('__INVOKE');
    echo $method->getNumberOfParameters(), ':', $method->getNumberOfRequiredParameters(), "\n";
    foreach ($view->getMethods() as $m) {
        if ($m->getName() === '__invoke') echo 'listed:', $m->getNumberOfParameters(), "\n";
    }
}
"#,
    r#"2:1
listed:2
3:2
listed:3
"#
);

oracle_case!(
    array_invoke_parameters_keep_the_selected_closure,
    r#"<?php
$f = function ($first, &$second = null) {};
foreach ([0, 1, 'first', 'second'] as $selector) {
    $p = new ReflectionParameter([$f, '__invoke'], $selector);
    $owner = $p->getDeclaringFunction();
    echo $p->getName(), ':', (int)$p->isOptional(), ':', (int)$p->isPassedByReference(), ':';
    echo get_class($owner), ':', $owner->getName(), "\n";
}
try { new ReflectionParameter([$f, '__invoke'], 'absent'); }
catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
echo (new ReflectionFunction($f))->getNumberOfParameters(), "\n";
"#,
    r#"first:0:0:ReflectionMethod:__invoke
second:1:1:ReflectionMethod:__invoke
first:0:0:ReflectionMethod:__invoke
second:1:1:ReflectionMethod:__invoke
The parameter specified by its name could not be found
2
"#
);

oracle_case!(
    bound_parameter_has_a_method_origin_without_class_table_pollution,
    r#"<?php
class BoundOrigin {}
$object = new BoundOrigin;
$f = (function ($suffix = '!') { return $suffix; })->bindTo($object, BoundOrigin::class);
$view = new ReflectionFunction($f);
$name = $view->getName();
$p = $view->getParameters()[0];
unset($view, $f);
$owner = $p->getDeclaringFunction();
echo get_class($owner), ':', (int)($owner->getName() === $name), ':', $owner->getDeclaringClass()->getName(), "\n";
echo count((new ReflectionClass($object))->getMethods()), "\n";
try { new ReflectionMethod($object, $name); }
catch (ReflectionException $e) { echo "not-registered\n"; }
"#,
    r#"ReflectionMethod:1:BoundOrigin
0
not-registered
"#
);

oracle_case!(
    ordinary_parameter_origins_are_not_closures,
    r#"<?php
function plainOrigin(&$input, $fallback = 9) { return $input; }
class MethodOrigin { private static function hidden($value) {} }
$p = new ReflectionParameter('plainOrigin', 1);
$fn = $p->getDeclaringFunction();
echo get_class($fn), ':', $fn->getName(), ':', (int)$p->isOptional(), "\n";
$p = new ReflectionParameter([MethodOrigin::class, 'hidden'], 0);
$fn = $p->getDeclaringFunction();
echo get_class($fn), ':', $fn->getName(), ':', (int)$fn->isPrivate(), ':', (int)$fn->isStatic(), "\n";
$p = (new ReflectionFunction('strlen'))->getParameters()[0];
echo $p->getDeclaringFunction()->getName(), "\n";
"#,
    r#"ReflectionFunction:plainOrigin:1
ReflectionMethod:hidden:1:1
strlen
"#
);

oracle_case!(
    declaring_method_factory_retains_captures_and_shared_static_cells,
    r#"<?php
class FactoryOrigin {}
$object = new FactoryOrigin;
$seed = 'kept';
$f = (function ($suffix) use ($seed) { static $n = 0; return $seed . ++$n . $suffix; })
    ->bindTo($object, FactoryOrigin::class);
$parameter = (new ReflectionFunction($f))->getParameters()[0];
$method = $parameter->getDeclaringFunction();
$g = $method->getClosure($object);
echo (int)($g === $f), ':', $g('!'), ':', $f('?'), ':', $g('.'), "\n";
$other = $method->getClosure(new FactoryOrigin);
echo $other('~'), ':', $f('='), "\n";
"#,
    r#"0:kept1!:kept2?:kept3.
kept4~:kept5=
"#
);

oracle_case!(
    reflected_invoke_closure_retains_cow_captures_after_release,
    r#"<?php
$items = ['first'];
$f = function ($value) use ($items) { $items[] = $value; return implode(',', $items); };
$method = new ReflectionMethod($f, '__invoke');
$call = $method->getClosure($f);
unset($method, $f);
$items[] = 'outside';
echo $call('one'), ':', $call('two'), ':', implode(',', $items), "\n";
"#,
    r#"first,one:first,two:first,outside
"#
);

oracle_case!(
    lexical_scope_and_called_class_remain_distinct,
    r#"<?php
class LexicalOrigin {
    private $secret = 'parent';
    function make() { return function () { return self::class . ':' . static::class . ':' . $this->secret; }; }
}
class CalledOrigin extends LexicalOrigin { private $secret = 'child'; }
$object = new CalledOrigin;
$f = $object->make();
echo $f(), "\n";
$bound = $f->bindTo($object, CalledOrigin::class);
echo $bound(), "\n", $f(), "\n";
"#,
    r#"LexicalOrigin:CalledOrigin:parent
CalledOrigin:CalledOrigin:child
LexicalOrigin:CalledOrigin:parent
"#
);

oracle_case!(
    explicit_null_scope_drops_private_access_without_mutating_original,
    r#"<?php
class NullOrigin {
    private static $secret = 5;
    static function make() { return static function () { return isset(NullOrigin::$secret); }; }
}
$f = NullOrigin::make();
$unscoped = $f->bindTo(null, null);
var_dump($f(), $unscoped(), $f());
$view = new ReflectionFunction($unscoped);
var_dump($view->getClosureScopeClass());
"#,
    r#"bool(true)
bool(false)
bool(true)
NULL
"#
);

oracle_case!(
    warmed_static_member_caches_do_not_transfer_private_capabilities,
    r#"<?php
class CachedOrigin {
    private static int $number = 5;
    private const LABEL = 'kept';
    static function reader() { return static function () { return CachedOrigin::$number; }; }
    static function writer() { return static function ($n) { CachedOrigin::$number = $n; }; }
    static function constantReader() { return static function () { $owner = CachedOrigin::class; return $owner::LABEL; }; }
    static function referencer() { return static function () { $n =& CachedOrigin::$number; return $n; }; }
}
$read = CachedOrigin::reader();
$write = CachedOrigin::writer();
$constant = CachedOrigin::constantReader();
$reference = CachedOrigin::referencer();
$n = 0;
echo $read(), ':', $constant(), ':', $reference($n), "\n";
$write(7);
foreach ([$read, $write, $constant, $reference] as $original) {
    $unscoped = $original->bindTo(null, null);
    try { $unscoped($n); echo "unexpected\n"; }
    catch (Error $e) { echo "blocked\n"; }
}
echo $read(), ':', $constant(), ':', $n, "\n";
"#,
    r#"5:kept:5
blocked
blocked
blocked
blocked
7:kept:0
"#
);

oracle_case!(
    nested_closure_inherits_the_rebound_lexical_scope,
    r#"<?php
class NestedOrigin {
    private static $secret = 'original';
    static function make() { return function () { return function () { return self::$secret; }; }; }
}
class NestedDestination { private static $secret = 'destination'; }
$outer = NestedOrigin::make();
$bound = $outer->bindTo(null, NestedDestination::class);
echo ($outer())(), ':', ($bound())(), ':', ($outer())(), "\n";
"#,
    r#"original:destination:original
"#
);

oracle_case!(
    nested_closure_retains_an_unread_bound_receiver,
    r#"<?php
class UnreadReceiver {}
$object = new UnreadReceiver;
$factory = function () { return function () { return 42; }; };
$bound = $factory->bindTo($object, UnreadReceiver::class);
$inner = $bound();
$view = new ReflectionFunction($inner);
var_dump($view->getClosureThis() === $object);
echo $view->getClosureScopeClass()->getName(), ':', $inner(), "\n";
var_dump((new ReflectionFunction($factory()))->getClosureThis());
unset($bound, $object);
echo get_class($view->getClosureThis()), "\n";
"#,
    r#"bool(true)
UnreadReceiver:42
NULL
UnreadReceiver
"#
);

oracle_case!(
    self_constants_follow_binding_and_preserve_original_static_state,
    r#"<?php
class ConstantOrigin { const LABEL = 'origin'; static function make() { return function () { static $n = 0; return self::LABEL . ':' . ++$n; }; } }
class ConstantDestination { const LABEL = 'destination'; }
$f = ConstantOrigin::make();
echo $f(), "\n";
$bound = $f->bindTo(null, ConstantDestination::class);
echo $bound(), ':', $f(), ':', $bound(), "\n";
"#,
    r#"origin:1
destination:2:origin:2:destination:3
"#
);

oracle_case!(
    internal_function_origin_rejects_scope_before_entry,
    r#"<?php
class InternalDestination {}
$f = (new ReflectionFunction('strlen'))->getClosure();
set_error_handler(function ($level, $message) { echo $message, "\n"; return true; });
function evaluatedArgument() { echo "argument\n"; return 'abc'; }
var_dump($f->call(new InternalDestination, evaluatedArgument()));
var_dump($f->bindTo(new InternalDestination, InternalDestination::class));
echo $f('abcd'), "\n";
restore_error_handler();
"#,
    r#"argument
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
Cannot rebind scope of closure created from function, this will be an error in PHP 9
NULL
4
"#
);

oracle_case!(
    inherited_static_closures_separate_self_parent_and_called_scope,
    r#"<?php
class ScopeRoot { const LABEL = 'root'; }
class ScopeBase extends ScopeRoot {
    const LABEL = 'base';
    static function make() { return static function () {
        return self::LABEL . ':' . parent::LABEL . ':' . static::LABEL;
    }; }
    static function invoke($f) { return $f(); }
}
class ScopeChild extends ScopeBase { const LABEL = 'child'; }
class ScopeOther extends ScopeRoot { const LABEL = 'other'; }
$f = ScopeChild::make();
$view = new ReflectionFunction($f);
echo $view->getClosureScopeClass()->getName(), ':', $view->getClosureCalledClass()->getName(), "\n";
echo $f(), "\n", $f->bindTo(null, 'static')(), "\n";
echo $f->bindTo(null, ScopeOther::class)(), "\n";
$unscoped = $f->bindTo(null, null);
var_dump((new ReflectionFunction($unscoped))->getClosureScopeClass());
try { ScopeBase::invoke($unscoped); } catch (Error $e) { echo "unscoped\n"; }
echo $f(), "\n";
"#,
    r#"ScopeBase:ScopeChild
base:root:child
base:root:base
other:root:other
NULL
unscoped
base:root:child
"#
);

oracle_case!(
    unread_receiver_carrier_can_be_unbound_but_direct_this_access_cannot,
    r#"<?php
class DetachOrigin {}
set_error_handler(function ($n, $message) { echo $message, "\n"; return true; });
foreach ([
    function () { return 7; },
    function () { return function () { return $this; }; },
    function () { return $this; },
    function () { return isset($this); }
] as $i => $f) {
    $object = new DetachOrigin;
    $bound = $f->bindTo($object, null);
    $detached = $bound->bindTo(null, null);
    echo $i, ':', $detached === null ? 'rejected' : 'detached', ':';
    echo (int)((new ReflectionFunction($bound))->getClosureThis() === $object), "\n";
}
restore_error_handler();
"#,
    r#"0:detached:1
1:detached:1
Cannot unbind $this of closure using $this, this will be an error in PHP 9
2:rejected:1
Cannot unbind $this of closure using $this, this will be an error in PHP 9
3:rejected:1
"#
);

oracle_case!(
    closure_scope_survives_wide_frames_reentry_and_extra_arguments,
    r#"<?php
class WideScopeBase {
    const LABEL = 'base';
    static function make($width) {
        $body = 'return static function ($callback = null) {';
        for ($i = 0; $i < $width; ++$i) $body .= '$local' . $i . ' = ' . $i . ';';
        $body .= 'if ($callback) { try { $callback(); } catch (Exception $e) {} }';
        $body .= 'return self::LABEL . ":" . static::LABEL; };';
        return eval($body);
    }
}
class WideScopeChild extends WideScopeBase { const LABEL = 'child'; }
class WideScopeOther { const LABEL = 'other'; }
foreach ([0, 40, 80] as $width) {
    $f = WideScopeChild::make($width);
    $g = $f->bindTo(null, WideScopeOther::class);
    $nested = function () use ($g) { echo $g(), ':'; throw new Exception('caught'); };
    echo $f($nested, 1, 2, 3, 4, 5), ':';
    echo (new ReflectionFunction($f))->invokeArgs([null, 1, 2, 3]), ':';
    echo call_user_func($g), "\n";
}
"#,
    r#"other:other:base:child:base:child:other:other
other:other:base:child:base:child:other:other
other:other:base:child:base:child:other:other
"#
);

oracle_case!(
    method_closure_scope_uses_the_trait_composition_not_the_trait_declaration,
    r#"<?php
trait CallableOriginTrait {
    function instanceOrigin() { return 1; }
    static function staticOrigin() { return 2; }
}
class CallableOriginBase { use CallableOriginTrait; }
class CallableOriginChild extends CallableOriginBase {}
foreach ([
    Closure::fromCallable([new CallableOriginChild, 'instanceOrigin']),
    Closure::fromCallable([CallableOriginChild::class, 'staticOrigin'])
] as $f) {
    $view = new ReflectionFunction($f);
    echo $view->getClosureScopeClass()->getName(), ':', $view->getClosureCalledClass()->getName(), ':', $f(), "\n";
}
"#,
    r#"CallableOriginBase:CallableOriginChild:1
CallableOriginBase:CallableOriginChild:2
"#
);

oracle_case!(
    extra_and_variadic_arguments_cannot_replace_the_bound_receiver,
    r#"<?php
class ArgumentReceiverBase {
    private $number = 7;
    function make() { return function ($first = 0) { return $this->number; }; }
    function variadic() { return function ($first, ...$tail) {
        return self::class . ':' . static::class . ':' . $this->number . ':' . count($tail);
    }; }
}
class ArgumentReceiverChild extends ArgumentReceiverBase { private $number = 9; }
$object = new ArgumentReceiverChild;
$f = $object->make();
echo $f(1), ':', $f(1, 2, 3), ':', $f(...[1, 2, 3]), "\n";
$g = $object->variadic();
echo $g(1, 2, 3), "\n";
echo call_user_func_array($g, [1, 2, 3]), "\n";
echo (new ReflectionFunction($g))->invokeArgs([1, 2, 3]), "\n";
echo $g(first: 1, extra: 2), "\n";
$bound = $g->bindTo($object, ArgumentReceiverChild::class);
echo $bound(1, 2), "\n", $g(1), "\n";
"#,
    r#"7:7:7
ArgumentReceiverBase:ArgumentReceiverChild:7:2
ArgumentReceiverBase:ArgumentReceiverChild:7:2
ArgumentReceiverBase:ArgumentReceiverChild:7:2
ArgumentReceiverBase:ArgumentReceiverChild:7:1
ArgumentReceiverChild:ArgumentReceiverChild:9:1
ArgumentReceiverBase:ArgumentReceiverChild:7:0
"#
);

oracle_case!(
    receiver_and_scope_carriers_are_not_user_variables,
    r#"<?php
class ScopeVisibilityObject {}
$object = new ScopeVisibilityObject;
$unused = (function () { return get_defined_vars(); })->bindTo($object, null);
$used = (function () {
    $visible = $this;
    return [array_keys(get_defined_vars()), compact('this')['this'] === $visible];
})->bindTo($object, null);
echo count($unused()), "\n";
[$names, $same] = $used();
echo implode(',', $names), ':', (int)$same, "\n";
echo (int)((new ReflectionFunction($unused))->getClosureThis() === $object), "\n";
"#,
    r#"0
visible:1
1
"#
);

oracle_case!(
    dummy_scope_is_lexical_and_survives_nested_and_detached_callbacks,
    r#"<?php
class BlankScopeHost {}
$factory = function () { return static function () { return self::class; }; };
$bound = $factory->bindTo(new BlankScopeHost, null);
foreach ([
    'bound' => $bound,
    'nested' => $bound(),
    'callback' => call_user_func($bound),
    'detached' => $bound->bindTo(null),
    'explicit' => $bound->bindTo(null, null),
] as $label => $closure) {
    $view = new ReflectionFunction($closure);
    echo $label, ':', $view->getClosureScopeClass()?->getName() ?? 'none', ':',
        $view->getClosureCalledClass()?->getName() ?? 'none', ':',
        $view->getClosureThis() === null ? 'no-this' : 'this', "\n";
}
echo $bound()(), ':', call_user_func(call_user_func($bound)), "\n";
"#,
    r#"bound:Closure:BlankScopeHost:this
nested:Closure:BlankScopeHost:no-this
callback:Closure:BlankScopeHost:no-this
detached:Closure:Closure:no-this
explicit:none:none:no-this
Closure:Closure
"#
);

oracle_case!(
    nested_static_closure_keeps_published_called_scope_without_inheriting_foreign_calls,
    r#"<?php
class NestedCalled { static function invoke($factory) { return $factory(); } }
$factory = function () { return static function () {}; };
$direct = $factory();
$foreign = NestedCalled::invoke($factory);
$dummy = $factory->bindTo(new NestedCalled, null);
foreach ([$direct, $foreign, $dummy()] as $closure) {
    $called = (new ReflectionFunction($closure))->getClosureCalledClass();
    echo $called?->getName() ?? 'none', "\n";
}
"#,
    r#"none
none
NestedCalled
"#
);
