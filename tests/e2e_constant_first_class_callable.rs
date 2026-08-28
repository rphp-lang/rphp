mod common;

use common::run_php;

#[test]
fn assertion_source_keeps_attributes_on_anonymous_callable_expressions() {
    assert_eq!(
        run_php(
            r#"<?php
#[Attribute]
class FccMarker { public function __construct(public mixed $callback) {} }
foreach ([
    fn() => assert(!#[FccMarker(strrev(...))] function () {}),
    fn() => assert(!new #[FccMarker(strrev(...))] class {}),
] as $check) {
    try { $check(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        "assert(!#[FccMarker(strrev(...))] function () {\n})\n\
assert(!new #[FccMarker(strrev(...))] class {\n})\n"
    );
}

#[test]
fn deferred_static_callable_autoloads_before_materialization_and_invokes() {
    assert_eq!(
        run_php(
            r#"<?php
namespace DeferredFcc;
$events = [];
\spl_autoload_register(static function (string $class) use (&$events): void {
    $events[] = "load:{$class}";
    if ($class === __NAMESPACE__ . '\\Target') {
        eval('namespace DeferredFcc; class Target { public static function run(string $value): string { return "ran:" . $value; } }');
    }
});
const CALLBACK = Target::run(...);
$events[] = 'declared';
$callback = CALLBACK;
echo implode('|', $events), '|', $callback('ok');
"#,
        ),
        "load:DeferredFcc\\Target|declared|ran:ok"
    );
}

#[test]
fn failed_callable_autoload_preserves_exception_and_retries_cleanly() {
    assert_eq!(
        run_php(
            r#"<?php
$attempts = 0;
spl_autoload_register(static function (string $class) use (&$attempts): void {
    if ($class === 'DeferredFailure') {
        ++$attempts;
        throw new RuntimeException("load-{$attempts}");
    }
});
function consume(Closure $callback = DeferredFailure::run(...)): void { echo 'body'; }
for ($round = 0; $round < 2; ++$round) {
    try { consume(); } catch (RuntimeException $error) {
        echo $error->getMessage(), ':', $attempts, "\n";
    }
}
"#,
        ),
        "load-1:1\nload-2:2\n"
    );
}

#[test]
fn abstract_static_callable_reports_abstract_error_at_default_materialization() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class DeferredAbstract {
    abstract public static function run(string $value): string;
}
function consumeAbstract(Closure $callback = DeferredAbstract::run(...)): void { echo 'body'; }
try { consumeAbstract(); } catch (Error $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "Error:Cannot call abstract method DeferredAbstract::run()"
    );
}

#[test]
fn constant_magic_static_callable_is_rejected_without_breaking_runtime_magic() {
    assert_eq!(
        run_php(
            r#"<?php
class DeferredMagic {
    public static function __callStatic(string $name, array $arguments): string {
        return $name . ':' . implode(',', $arguments);
    }
}
function consumeMagic(Closure $callback = DeferredMagic::missing(...)): void { echo 'body'; }
try { consumeMagic(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
$runtime = DeferredMagic::missing(...);
echo $runtime('x', 'y');
"#,
        ),
        "Creating a callable for the magic __callStatic() method is not supported in constant expressions\nmissing:x,y"
    );
}

#[test]
fn namespace_fallback_resolution_is_stable_after_conditional_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
namespace DeferredNamespace;
function inspect(\Closure $default = strrev(...)): void {
    $runtime = strrev(...);
    echo $default('abc'), ':', $runtime('abc'), "\n";
}
inspect();
if (true) {
    function strrev(string $value): string { return "local:{$value}"; }
}
inspect();
"#,
        ),
        "cba:cba\ncba:cba\n"
    );
}

#[test]
fn deferred_callable_identity_and_cow_match_each_materialization_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
const NAMED_CALLBACK = strlen(...);
$first = NAMED_CALLBACK;
$second = NAMED_CALLBACK;
$defaults = [];
function retainDefault(Closure $callback = strlen(...)): void {
    global $defaults;
    $defaults[] = $callback;
}
retainDefault(); retainDefault();
class CallableProperty { public Closure $callback = strlen(...); }
$left = new CallableProperty;
$right = new CallableProperty;
$copy = $first;
$copy = static fn(string $value): int => 99;
echo ($first === $second ? 'const-same' : 'const-new'), '|';
echo ($defaults[0] === $defaults[1] ? 'default-same' : 'default-new'), '|';
echo ($left->callback === $right->callback ? 'property-same' : 'property-new'), '|';
echo $first('four'), ':', $copy('x');
"#,
        ),
        "const-same|default-new|property-same|4:99"
    );
}

#[test]
fn attribute_callable_materialization_preserves_scope_autoload_and_fresh_identity() {
    assert_eq!(
        run_php(
            r#"<?php
#[Attribute(Attribute::TARGET_CLASS | Attribute::IS_REPEATABLE)]
class CallableArgument { public function __construct(public Closure $callback) {} }
$events = [];
spl_autoload_register(static function (string $class) use (&$events): void {
    $events[] = "load:{$class}";
    if ($class === 'DeferredAttributeTarget') {
        eval('class DeferredAttributeTarget { public static function run(string $value): string { return "loaded:" . $value; } }');
    }
});
#[CallableArgument(DeferredAttributeTarget::run(...))]
class AttributeOwner {}
$attribute = (new ReflectionClass(AttributeOwner::class))->getAttributes()[0];
echo implode('|', $events), ":before\n";
$first = $attribute->getArguments()[0];
$second = $attribute->getArguments()[0];
echo implode('|', $events), ':after:', ($first === $second ? 'same' : 'fresh'), ':', $first('x'), "\n";

#[CallableArgument(ScopedAttributeOwner::hidden(...))]
#[CallableArgument(ScopedAttributeOwner::missing(...))]
class ScopedAttributeOwner {
    private static function hidden(string $value): string { return "private:{$value}"; }
    public static function __callStatic(string $name, array $arguments): string { return 'mutated'; }
}
$scoped = (new ReflectionClass(ScopedAttributeOwner::class))->getAttributes();
echo $scoped[0]->getArguments()[0]('ok'), "\n";
try { $scoped[1]->getArguments(); } catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        ":before\n\
load:DeferredAttributeTarget:after:fresh:loaded:x\n\
private:ok\n\
Creating a callable for the magic __callStatic() method is not supported in constant expressions"
    );
}

#[test]
fn dynamic_static_callable_autoload_precedes_member_evaluation_and_failures() {
    assert_eq!(
        run_php(
            r#"<?php
spl_autoload_register(static function (string $class): void {
    echo "load:{$class}>";
    if ($class === 'OrderedCallable') {
        eval('class OrderedCallable { public static function run(): void { echo "call\n"; } }');
    }
});
function owner(string $name): string { echo "owner:{$name}>"; return $name; }
function member(): string { echo "member>"; return 'run'; }
$callback = (owner('OrderedCallable'))::{member()}(...);
$callback();
try { (owner('MissingCallable'))::{member()}(...); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
function invalidOwner(): int { echo 'invalid>'; return 42; }
try { (invalidOwner())::{member()}(...); }
catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        "owner:OrderedCallable>load:OrderedCallable>member>call\n\
owner:MissingCallable>load:MissingCallable>Class \"MissingCallable\" not found\n\
invalid>Class name must be a valid object or a string"
    );
}
