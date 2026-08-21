mod common;
use common::{run_php, run_php_expect_error, run_php_with_source_context};

#[test]
fn reflection_class_get_name_returns_the_declared_qualified_name() {
    assert_eq!(
        run_php(
            "<?php namespace Reflected\\Names; class NamedProbe {} echo (new \\ReflectionClass(NamedProbe::class))->getName();"
        ),
        "Reflected\\Names\\NamedProbe"
    );
}

#[test]
fn reflection_doc_comments_truthfully_report_unretained_metadata() {
    assert_eq!(
        run_php(
            "<?php /** class docs */ class Documented { /** property docs */ public int $value; /** method docs */ public function read(): void {} } $class = new ReflectionClass(Documented::class); $properties = $class->getProperties(); var_dump($class->getDocComment(), $class->getMethod('read')->getDocComment(), $properties[0]->getDocComment());"
        ),
        "bool(false)\nbool(false)\nbool(false)\n"
    );
}

#[test]
fn reflection_class_is_subclass_of_accepts_names_and_reflections() {
    assert_eq!(
        run_php(
            "<?php interface ReflectedMarker {} class ReflectedBase implements ReflectedMarker {} class ReflectedChild extends ReflectedBase {} $child = new ReflectionClass(ReflectedChild::class); echo $child->isSubclassOf(ReflectedBase::class) ? 'parent:' : 'bad:'; echo $child->isSubclassOf(new ReflectionClass(ReflectedMarker::class)) ? 'interface:' : 'bad:'; echo (new ReflectionClass(ReflectedBase::class))->isSubclassOf(ReflectedBase::class) ? 'bad' : 'same';"
        ),
        "parent:interface:same"
    );
}

#[test]
fn reflection_class_implements_interface_includes_inherited_and_interface_identity() {
    assert_eq!(
        run_php(
            "<?php interface RootContract {} interface ChildContract extends RootContract {} class ContractParent implements ChildContract {} class ContractChild extends ContractParent {} $child = new ReflectionClass(ContractChild::class); echo (int) $child->implementsInterface(RootContract::class), (int) $child->implementsInterface(ChildContract::class), ':'; echo (int) (new ReflectionClass(RootContract::class))->implementsInterface(RootContract::class);"
        ),
        "11:1"
    );
}

#[test]
fn reflection_class_get_interfaces_and_traits_return_named_reflections() {
    assert_eq!(
        run_php(
            "<?php interface ObjectRoot {} interface ObjectChild extends ObjectRoot {} trait ObjectTrait {} class ObjectParent implements ObjectChild {} class ObjectLeaf extends ObjectParent { use ObjectTrait; } $reflection = new ReflectionClass(ObjectLeaf::class); foreach ($reflection->getInterfaces() as $name => $interface) { echo $name, '=', $interface->getName(), ','; } echo '|'; foreach ($reflection->getTraits() as $name => $trait) { echo $name, '=', $trait->getName(); }"
        ),
        "ObjectChild=ObjectChild,ObjectRoot=ObjectRoot,|ObjectTrait=ObjectTrait"
    );
}

#[test]
fn reflection_class_get_constructor_reports_inherited_or_missing_constructor() {
    assert_eq!(
        run_php(
            "<?php class ConstructorOwner { protected function __construct(int $value) {} } class ConstructorChild extends ConstructorOwner {} class ConstructorMissing {} $constructor = (new ReflectionClass(ConstructorChild::class))->getConstructor(); echo $constructor->getName(), ':', $constructor->getDeclaringClass()->getName(), ':', $constructor->getModifiers(), ':'; var_dump((new ReflectionClass(ConstructorMissing::class))->getConstructor());"
        ),
        "__construct:ConstructorOwner:2:NULL\n"
    );
}

#[test]
fn reflection_method_get_prototype_and_invoke_follow_parent_contract() {
    assert_eq!(
        run_php(
            "<?php class PrototypeParent { public function render($value) { return 'P'.$value; } } class PrototypeChild extends PrototypeParent { public function render($value) { return 'C'.$value; } } $method = new ReflectionMethod(PrototypeChild::class, 'render'); echo (int) $method->hasPrototype(), ':', $method->getPrototype()->getDeclaringClass()->getName(), ':', $method->invoke(new PrototypeChild(), 'x');"
        ),
        "1:PrototypeParent:Cx"
    );
}

#[test]
fn reflection_method_can_reflect_and_invoke_internal_methods() {
    assert_eq!(
        run_php(
            "<?php $method = new ReflectionMethod(ReflectionMethod::class, 'getPrototype'); echo $method->getName(), ':', $method->getDeclaringClass()->getName(), ':'; $target = new ReflectionMethod(ReflectionMethod::class, 'getPrototype'); try { $method->invoke($target); } catch (ReflectionException $error) { echo 'caught'; }"
        ),
        "getPrototype:ReflectionMethod:caught"
    );
}

#[test]
fn reflection_method_visibility_and_declaration_predicates_share_metadata() {
    assert_eq!(
        run_php(
            "<?php class ReflectedPredicates { protected function pending() {} final public static function ready() {} private function __destruct() {} } foreach ((new ReflectionClass(ReflectedPredicates::class))->getMethods() as $method) { echo $method->getName(), ':', (int) $method->isPublic(), (int) $method->isProtected(), (int) $method->isPrivate(), (int) $method->isStatic(), (int) $method->isFinal(), (int) $method->isAbstract(), (int) $method->isDestructor(), '|'; }"
        ),
        "pending:0100000|ready:1001100|__destruct:0010001|"
    );
}

#[test]
fn reflection_class_method_lookup_and_kind_predicates_are_consistent() {
    assert_eq!(
        run_php(
            "<?php interface LookupInterface {} trait LookupTrait { public function fromTrait() {} } abstract class LookupParent { use LookupTrait; protected function inherited() {} } final class LookupChild extends LookupParent {} $class = new ReflectionClass(LookupChild::class); echo (int) $class->hasMethod('FROMTRAIT'), ':', $class->getMethod('inherited')->getDeclaringClass()->getName(), ':', (int) $class->isFinal(), (int) $class->isAbstract(), (int) $class->isInstantiable(), ':'; echo (int) (new ReflectionClass(LookupInterface::class))->isInterface(), (int) (new ReflectionClass(LookupTrait::class))->isTrait();"
        ),
        "1:LookupParent:101:11"
    );
}

#[test]
fn reflection_class_reports_class_level_readonly_metadata() {
    assert_eq!(
        run_php(
            "<?php readonly final class ReadonlyClassProbe { public function __construct(public int $value) {} } class MutableClassProbe { public int $value = 0; } $readonly = new ReflectionClass(ReadonlyClassProbe::class); $mutable = new ReflectionClass(MutableClassProbe::class); echo (int) $readonly->isReadOnly(), (int) $readonly->isFinal(), ':', (int) $readonly->getProperties()[0]->isReadOnly(), ':', (int) $mutable->isReadOnly();"
        ),
        "11:1:0"
    );
}

#[test]
fn reflection_attributes_preserve_names_arguments_targets_and_instances() {
    assert_eq!(
        run_php(
            "<?php namespace Metadata; #[\\Attribute(\\Attribute::TARGET_ALL | \\Attribute::IS_REPEATABLE)] class Label { public function __construct(public string $name, public int $rank = 7) {} } #[Label('class'), Label(name: 'again', rank: 9)] class Subject { #[Label('constant')] public const TOKEN = 1; #[Label('property')] public string $value; #[Label('method')] public function run(#[Label('parameter')] $input): void {} } #[Label('function')] function helper() {} #[Label('global')] const GLOBAL_TOKEN = 1; $class = new \\ReflectionClass(Subject::class); $first = $class->getAttributes()[0]; $second = $class->getAttributes()[1]; $instance = $second->newInstance(); echo $first->getName(), ':', $first->getArguments()[0], ':', $first->getTarget(), ':', (int) $first->isRepeated(), '|'; echo $instance->name, ':', $instance->rank, '|'; echo $class->getReflectionConstant('TOKEN')->getAttributes()[0]->getTarget(), ','; echo $class->getProperty('value')->getAttributes()[0]->getTarget(), ','; echo $class->getMethod('run')->getAttributes()[0]->getTarget(), ','; echo $class->getMethod('run')->getParameters()[0]->getAttributes()[0]->getTarget(), ','; echo (new \\ReflectionFunction(__NAMESPACE__ . '\\\\helper'))->getAttributes()[0]->getTarget(), ','; echo (new \\ReflectionConstant(__NAMESPACE__ . '\\\\GLOBAL_TOKEN'))->getAttributes()[0]->getTarget();"
        ),
        "Metadata\\Label:class:1:1|again:9|16,8,4,32,2,64"
    );
}

#[test]
fn reflection_attribute_constructor_trace_retains_use_site_and_internal_trampoline() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
#[Attribute]
class TraceOriginLabel {
    public function __construct() {
        $trace = debug_backtrace(DEBUG_BACKTRACE_PROVIDE_OBJECT);
        echo $trace[0]['file'], ':', $trace[0]['line'], ':', $trace[0]['function'], ':', $trace[0]['class'], ':', get_class($trace[0]['object']), '|';
        echo $trace[1]['function'], ':', $trace[1]['class'], ':', get_class($trace[1]['object']);
    }
}
#[TraceOriginLabel]
class TraceOriginTarget {}
(new ReflectionClass(TraceOriginTarget::class))->getAttributes()[0]->newInstance();
"#,
            "/app/attribute-trace.php",
            "/app",
        ),
        "/app/attribute-trace.php:10:__construct:TraceOriginLabel:TraceOriginLabel|newInstance:ReflectionAttribute:ReflectionAttribute"
    );
}

#[test]
fn strict_attribute_argument_error_snapshots_the_pending_constructor_call() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
declare(strict_types=1);
#[Attribute]
class StrictOriginLabel {
    public function __construct(public int $value) {}
}
#[StrictOriginLabel('9')]
class StrictOriginTarget {}
try {
    (new ReflectionClass(StrictOriginTarget::class))->getAttributes()[0]->newInstance();
} catch (TypeError $error) {
    $trace = $error->getTrace();
    echo $error->getFile(), ':', $error->getLine(), '|';
    echo $trace[0]['file'], ':', $trace[0]['line'], ':', $trace[0]['function'], ':', $trace[0]['args'][0], '|';
    echo $trace[1]['function'];
}
"#,
            "/app/attribute-strict.php",
            "/app",
        ),
        "/app/attribute-strict.php:5|/app/attribute-strict.php:7:__construct:9|newInstance"
    );
}

#[test]
fn reflection_attribute_filtering_and_validation_are_deferred_until_instantiation() {
    assert_eq!(
        run_php(
            "<?php #[Attribute(Attribute::TARGET_FUNCTION)] class FunctionLabel { public function __construct(public int $value = 5) {} } class PlainLabel {} #[FunctionLabel(11)] function valid() {} #[FunctionLabel] class WrongTarget {} #[PlainLabel] function wrongClass() {} $valid = (new ReflectionFunction('valid'))->getAttributes(FunctionLabel::class, ReflectionAttribute::IS_INSTANCEOF)[0]; echo $valid->newInstance()->value, '|'; try { (new ReflectionClass(WrongTarget::class))->getAttributes()[0]->newInstance(); } catch (Error $error) { echo $error->getMessage(), '|'; } try { (new ReflectionFunction('wrongClass'))->getAttributes()[0]->newInstance(); } catch (Error $error) { echo $error->getMessage(); }"
        ),
        "11|Attribute \"FunctionLabel\" cannot target class (allowed targets: function)|Attempting to use non-attribute class \"PlainLabel\" as attribute"
    );
}

#[test]
fn attribute_marker_constructs_flags_and_validates_reflected_declarations() {
    assert_eq!(
        run_php(
            "<?php $default = new Attribute(); echo $default->flags, '|'; $marker = (new ReflectionClass(Attribute::class))->getAttributes()[0]->newInstance(); echo get_class($marker), ':', $marker->flags, '|'; #[Attribute('bad')] class InvalidType {} #[InvalidType] class TypeTarget {} try { (new ReflectionClass(TypeTarget::class))->getAttributes()[0]->newInstance(); } catch (TypeError $error) { echo $error->getMessage(), '|'; } #[Attribute(-1)] class InvalidFlags {} #[InvalidFlags] class FlagsTarget {} try { (new ReflectionClass(FlagsTarget::class))->getAttributes()[0]->newInstance(); } catch (Error $error) { echo $error->getMessage(); }"
        ),
        "127|Attribute:1|Attribute::__construct(): Argument #1 ($flags) must be of type int, string given|Invalid attribute flags specified"
    );
}

#[test]
fn attribute_marker_validates_declaration_targets_and_repetition_at_compile_time() {
    let cases = [
        (
            "<?php #[Attribute] abstract class Invalid {}",
            "Cannot apply #[\\Attribute] to abstract class Invalid",
        ),
        (
            "<?php namespace Domain; use Attribute as Marker; #[Marker] abstract class Invalid {}",
            "Cannot apply #[\\Attribute] to abstract class Domain\\Invalid",
        ),
        (
            "<?php #[Attribute] interface Invalid {}",
            "Cannot apply #[\\Attribute] to interface Invalid",
        ),
        (
            "<?php #[Attribute] trait Invalid {}",
            "Cannot apply #[\\Attribute] to trait Invalid",
        ),
        (
            "<?php #[Attribute] enum Invalid {}",
            "Cannot apply #[\\Attribute] to enum Invalid",
        ),
        (
            "<?php #[Attribute] function invalid() {}",
            "Attribute \"Attribute\" cannot target function (allowed targets: class)",
        ),
        (
            "<?php #[Attribute] const INVALID = 1;",
            "Attribute \"Attribute\" cannot target constant (allowed targets: class)",
        ),
        (
            "<?php class Invalid { #[Attribute] public const VALUE = 1; }",
            "Attribute \"Attribute\" cannot target class constant (allowed targets: class)",
        ),
        (
            "<?php class Invalid { #[Attribute] public int $value; }",
            "Attribute \"Attribute\" cannot target property (allowed targets: class)",
        ),
        (
            "<?php class Invalid { #[Attribute] public function method() {} }",
            "Attribute \"Attribute\" cannot target method (allowed targets: class)",
        ),
        (
            "<?php function invalid(#[Attribute] $value) {}",
            "Attribute \"Attribute\" cannot target parameter (allowed targets: class)",
        ),
        (
            "<?php $closure = #[Attribute] function () {};",
            "Attribute \"Attribute\" cannot target function (allowed targets: class)",
        ),
        (
            "<?php #[Attribute] #[Attribute] class Invalid {}",
            "Attribute \"Attribute\" must not be repeated",
        ),
        (
            "<?php #[DelayedTargetValidation] #[Attribute] #[Attribute] function invalid() {}",
            "Attribute \"Attribute\" must not be repeated",
        ),
    ];
    for (source, expected) in cases {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected Attribute validation error: {error}"
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
namespace {
#[Attribute] class PlainAttribute {}
#[Attribute] final class FinalAttribute {}
#[Attribute] readonly class ReadonlyAttribute {}
$anonymous = new #[Attribute] class {};
}
namespace Domain {
#[Attribute] abstract class LocalAttribute {}
#[\DelayedTargetValidation] #[\Attribute] trait DelayedTrait {}
#[\DelayedTargetValidation] #[\Attribute] function delayed() {}
echo 'ok';
}
"#,
        ),
        "ok"
    );
}

#[test]
fn deprecated_attribute_validates_declaration_targets_and_repetition_at_compile_time() {
    let cases = [
        (
            "<?php function invalid(#[Deprecated] $value) {}",
            "Attribute \"Deprecated\" cannot target parameter (allowed targets: class, function, method, class constant, constant)",
        ),
        (
            "<?php class Invalid { #[Deprecated] public string $value; }",
            "Attribute \"Deprecated\" cannot target property (allowed targets: class, function, method, class constant, constant)",
        ),
        (
            "<?php #[Deprecated] #[Deprecated] function invalid() {}",
            "Attribute \"Deprecated\" must not be repeated",
        ),
        (
            "<?php #[DelayedTargetValidation] #[Deprecated] #[Deprecated] function invalid() {}",
            "Attribute \"Deprecated\" must not be repeated",
        ),
        (
            "<?php #[Deprecated] class Invalid {}",
            "Cannot apply #[\\Deprecated] to class Invalid",
        ),
        (
            "<?php #[Deprecated] interface Invalid {}",
            "Cannot apply #[\\Deprecated] to interface Invalid",
        ),
        (
            "<?php #[Deprecated] enum Invalid {}",
            "Cannot apply #[\\Deprecated] to enum Invalid",
        ),
    ];
    for (source, expected) in cases {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected Deprecated validation error: {error}"
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
namespace {
#[Deprecated] trait DeprecatedTrait {}
#[DelayedTargetValidation] #[Deprecated] class DelayedClass {}
#[DelayedTargetValidation] #[Deprecated] interface DelayedInterface {}
#[DelayedTargetValidation] #[Deprecated] enum DelayedEnum {}
class DelayedMembers {
    #[DelayedTargetValidation] #[Deprecated] public string $value;
    public function method(#[DelayedTargetValidation] #[Deprecated] $value) {}
}
}
namespace Domain {
#[\Attribute] class Deprecated {}
#[Deprecated] class LocalTarget {}
echo 'ok';
}
"#,
        ),
        "ok"
    );

    assert_eq!(
        run_php(
            r#"<?php
#[DelayedTargetValidation] #[Deprecated] class DelayedClass {}
#[DelayedTargetValidation] #[Deprecated] interface DelayedInterface {}
#[DelayedTargetValidation] #[Deprecated] enum DelayedEnum {}
#[DelayedTargetValidation] #[Deprecated] trait DelayedTrait {}
foreach ([DelayedClass::class, DelayedInterface::class, DelayedEnum::class] as $class) {
    try {
        (new ReflectionClass($class))->getAttributes(Deprecated::class)[0]->newInstance();
    } catch (Error $error) {
        echo $error->getMessage(), '|';
    }
}
echo get_class((new ReflectionClass(DelayedTrait::class))->getAttributes(Deprecated::class)[0]->newInstance());
"#,
        ),
        concat!(
            "Cannot apply #[\\Deprecated] to class DelayedClass|",
            "Cannot apply #[\\Deprecated] to interface DelayedInterface|",
            "Cannot apply #[\\Deprecated] to enum DelayedEnum|Deprecated",
        )
    );
}

#[test]
fn deprecated_attribute_reports_callable_names_messages_and_suppression() {
    assert_eq!(
        run_php(
            r#"<?php
$handler = function ($level, $message) {
    echo $level, ':', $message, '|';
    return true;
};
set_error_handler($handler);

#[Deprecated('use current', since: '8.5')]
function legacy_call($value) { echo "function:$value|"; }

class DeprecatedCallProbe {
    #[Deprecated('use run')]
    public function old() { echo "method|"; }

    #[Deprecated('use direct')]
    public function __call($name, $arguments) { echo "magic:$name|"; }
}

legacy_call(1);
restore_error_handler();
@legacy_call(2);
set_error_handler($handler);
$probe = new DeprecatedCallProbe();
$probe->old();
$probe->missing();
"#,
        ),
        concat!(
            "16384:Function legacy_call() is deprecated since 8.5, use current|",
            "function:1|function:2|",
            "16384:Method DeprecatedCallProbe::old() is deprecated, use run|method|",
            "16384:Method DeprecatedCallProbe::missing() is deprecated, use direct|magic:missing|",
        )
    );
}

#[test]
fn deprecated_exception_handler_uses_internal_origin_before_main_destructors() {
    assert_eq!(
        run_php(
            r#"<?php
class HandlerOrderProbe {
    public function __destruct() { echo '|destruct'; }
}

$probe = new HandlerOrderProbe();
set_error_handler(function ($level, $message, $file, $line) {
    echo $level, ':', $message, ':', $file, ':', $line, '|';
    return true;
});

#[Deprecated('use current')]
function legacy_exception_handler($exception) {
    echo get_exception_handler() === null ? 'null:' : 'registered:';
    echo $exception->getMessage();
}

set_exception_handler('legacy_exception_handler');
throw new Exception('handled');
"#,
        ),
        concat!(
            "16384:Function legacy_exception_handler() is deprecated, use current:Unknown:0|",
            "null:handled|destruct",
        )
    );
}

#[test]
fn native_enum_attribute_arguments_reach_deprecated_constructor_validation() {
    assert_eq!(
        run_php(
            r#"<?php
#[Deprecated(Random\IntervalBoundary::OpenClosed)]
function invalid_deprecated_metadata() {}

try {
    (new ReflectionFunction("invalid_deprecated_metadata"))->getAttributes()[0]->newInstance();
} catch (TypeError $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "Deprecated::__construct(): Argument #1 ($message) must be of type ?string, ",
            "Random\\IntervalBoundary given",
        )
    );
}

#[test]
fn reflection_method_on_callable_closure_preserves_source_deprecation_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
#[Deprecated('retire function')]
function reflected_legacy_function() {}

class ReflectedLegacyCallable {
    #[Deprecated('retire method')]
    public function __invoke() {}
}

$anonymous = function () {};
$function = Closure::fromCallable('reflected_legacy_function');
$method = Closure::fromCallable(new ReflectedLegacyCallable());
$method = $method->__invoke(...);

foreach ([$anonymous, $function, $method] as $closure) {
    $reflection = new ReflectionMethod($closure, '__invoke');
    echo $reflection->getName(), ':',
        $reflection->getDeclaringClass()->getName(), ':',
        (int) $reflection->isStatic(),
        (int) $reflection->isFinal(), ':',
        (int) $reflection->isDeprecated(), ':',
        count($reflection->getAttributes()), '|';
}
"#,
        ),
        "__invoke:Closure:00:0:0|__invoke:Closure:00:1:1|__invoke:Closure:00:1:1|"
    );
}

#[test]
fn deprecated_attribute_validates_at_call_time_and_thrown_handlers_abort_the_body() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);

#[Deprecated(1234)]
function invalid_deprecation() { echo 'invalid-body|'; }

try {
    invalid_deprecation();
} catch (TypeError $error) {
    echo $error->getMessage(), '|';
}

set_error_handler(function ($level, $message) {
    throw new Exception($message);
});

#[Deprecated('stop before entry')]
function aborted_deprecation() { echo 'aborted-body'; }

try {
    aborted_deprecation();
} catch (Exception $error) {
    echo 'caught:', $error->getMessage();
}
"#,
        ),
        concat!(
            "Deprecated::__construct(): Argument #1 ($message) must be of type ?string, int given|",
            "caught:Function aborted_deprecation() is deprecated, stop before entry",
        )
    );
}

#[test]
fn deprecated_builtin_preserves_readonly_and_constructorless_initialization_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$deprecated = new Deprecated('first', since: '8.5');
echo $deprecated->message, ':', $deprecated->since, '|';
try {
    $deprecated->__construct('again');
} catch (Error $error) {
    echo $error->getMessage(), '|';
}

$uninitialized = (new ReflectionClass(Deprecated::class))->newInstanceWithoutConstructor();
$uninitialized->__construct('late');
echo $uninitialized->message, ':', (int) (new ReflectionProperty(Deprecated::class, 'message'))->isReadOnly(), '|';
$marker = (new ReflectionClass(Deprecated::class))->getAttributes()[0]->newInstance();
echo get_class($marker), ':', $marker->flags;
"#,
        ),
        concat!(
            "first:8.5|",
            "Cannot modify readonly property Deprecated::$message|",
            "late:1|Attribute:87",
        )
    );
}

#[test]
fn no_discard_builtin_exposes_php_85_metadata_and_readonly_message() {
    assert_eq!(
        run_php(
            r#"<?php
$class = new ReflectionClass(NoDiscard::class);
$property = $class->getProperty('message');
$marker = $class->getAttributes()[0]->newInstance();
$empty = new NoDiscard();
$message = new NoDiscard('keep this');
echo $class->getName(), ':', (int) $class->isFinal(), (int) $class->isInternal(), '|';
echo $marker->flags, ':', (int) $property->isReadOnly(), ':';
var_dump($empty->message);
echo $message->message, '|';
try {
    $message->__construct('again');
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "NoDiscard:11|6:1:NULL\n",
            "keep this|Cannot modify readonly property NoDiscard::$message",
        )
    );
}

#[test]
fn sensitive_parameter_builtins_expose_php_85_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
$markerClass = new ReflectionClass(SensitiveParameter::class);
$markerAttribute = $markerClass->getAttributes()[0];
echo $markerClass->getName(), ':', (int) $markerClass->isInternal(), (int) $markerClass->isFinal(), ':';
echo $markerAttribute->getName(), ':', $markerAttribute->getArguments()[0], ':', get_class($markerAttribute->newInstance()), ':';
echo $markerClass->getConstructor()->getNumberOfParameters(), '|';

$valueClass = new ReflectionClass(SensitiveParameterValue::class);
$property = $valueClass->getProperty('value');
echo $valueClass->getName(), ':', (int) $valueClass->isInternal(), (int) $valueClass->isFinal(), ':';
echo (int) $property->isPrivate(), (int) $property->isReadOnly(), ':', $property->getType(), ':';
echo $valueClass->getConstructor()->getNumberOfRequiredParameters(), ':';
echo $valueClass->getMethod('getValue')->getReturnType(), ':';
echo $valueClass->getMethod('__debugInfo')->getReturnType();
"#,
        ),
        concat!(
            "SensitiveParameter:11:Attribute:32:Attribute:0|",
            "SensitiveParameterValue:11:11:mixed:1:mixed:array",
        )
    );
}

#[test]
fn no_discard_reports_only_unused_results_across_direct_and_callback_calls() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $level, ':', $message, '|';
    return true;
});

#[NoDiscard('retain function result')]
function retained() { echo 'function-body|'; return 1; }

trait RetainedTrait {
    #[NoDiscard]
    public function fromTrait() { echo 'trait-body|'; return 2; }
}
class RetainedCalls {
    use RetainedTrait;
    #[NoDiscard]
    public function method() { echo 'method-body|'; return 3; }
}

$closure = #[NoDiscard] function () { echo 'closure-body|'; return 4; };
$object = new RetainedCalls();
retained();
$_ = retained();
(void)retained();
call_user_func('retained');
$_ = call_user_func('retained');
(void)call_user_func_array('retained', []);
$object->method();
$object->fromTrait();
$closure();
"#,
        ),
        concat!(
            "512:The return value of function retained() should either be used or intentionally ignored by casting it as (void), retain function result|function-body|",
            "function-body|function-body|",
            "512:The return value of function retained() should either be used or intentionally ignored by casting it as (void), retain function result|function-body|",
            "function-body|function-body|",
            "512:The return value of method RetainedCalls::method() should either be used or intentionally ignored by casting it as (void)|method-body|",
            "512:The return value of method RetainedCalls::fromTrait() should either be used or intentionally ignored by casting it as (void)|trait-body|",
            "512:The return value of function {closure}() should either be used or intentionally ignored by casting it as (void)|closure-body|",
        )
    );
}

#[test]
fn no_discard_validation_rejects_unsupported_declarations_before_execution() {
    let cases = [
        (
            "<?php #[NoDiscard] function invalid(): void {}",
            "A void function does not return a value, but #[\\NoDiscard] requires a return value",
        ),
        (
            "<?php #[NoDiscard] function invalid(): never { throw new Exception(); }",
            "A never returning function does not return a value, but #[\\NoDiscard] requires a return value",
        ),
        (
            "<?php class Invalid { #[NoDiscard] function __construct() {} }",
            "Method Invalid::__construct cannot be #[\\NoDiscard]",
        ),
        (
            "<?php class Invalid { public string $value { #[NoDiscard] get => 'x'; } }",
            "#[\\NoDiscard] is not supported for property hooks",
        ),
        (
            "<?php #[DelayedTargetValidation] #[NoDiscard] #[NoDiscard] class Invalid {}",
            "Attribute \"NoDiscard\" must not be repeated",
        ),
    ];

    for (source, expected) in cases {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected NoDiscard validation error: {error}"
        );
    }
}

#[test]
fn override_is_an_internal_method_and_property_attribute() {
    assert_eq!(
        run_php(
            r#"<?php
$class = new ReflectionClass(Override::class);
$marker = $class->getAttributes(Attribute::class, ReflectionAttribute::IS_INSTANCEOF)[0];
echo $class->getName(), ':', (int) $class->isInternal(), (int) $class->isFinal(), (int) $class->isInstantiable(), ':';
echo $marker->getName(), ':', $marker->getArguments()[0], ':';
$constructor = $class->getConstructor();
echo $constructor->getName(), ':', $constructor->getNumberOfParameters(), ':';
echo get_class(new Override), ':';

class OverrideBase { public function inherited(): void {} }
class OverrideChild extends OverrideBase {
    #[Override]
    public function inherited(): void {}
}
$attribute = (new ReflectionMethod(OverrideChild::class, 'inherited'))->getAttributes()[0];
echo $attribute->getTarget(), ':';
echo get_class($attribute->newInstance());
"#,
        ),
        "Override:111:Attribute:12:__construct:0:Override:4:Override"
    );
}

#[test]
fn override_target_and_repetition_validation_precedes_execution() {
    let cases = [
        (
            "<?php #[Override] class Invalid {}",
            "Attribute \"Override\" cannot target class (allowed targets: method, property)",
        ),
        (
            "<?php #[Override] function invalid() {}",
            "Attribute \"Override\" cannot target function (allowed targets: method, property)",
        ),
        (
            "<?php class Invalid { #[Override] public const VALUE = 1; }",
            "Attribute \"Override\" cannot target class constant (allowed targets: method, property)",
        ),
        (
            "<?php function invalid(#[Override] $value) {}",
            "Attribute \"Override\" cannot target parameter (allowed targets: method, property)",
        ),
        (
            "<?php class ParentType { public function value() {} } class Invalid extends ParentType { #[Override] #[Override] public function value() {} }",
            "Attribute \"Override\" must not be repeated",
        ),
    ];
    for (source, expected) in cases {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected Override validation error: {error}"
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
#[DelayedTargetValidation]
#[Override]
class DelayedClass {}
#[DelayedTargetValidation]
#[Override]
function delayed(#[DelayedTargetValidation] #[Override] $value): void {}
echo 'ok';
"#,
        ),
        "ok"
    );
}

#[test]
fn property_hook_type_exposes_php_85_backed_enum_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$class = new ReflectionClass(PropertyHookType::class);
echo $class->getName(), ':', (int) enum_exists(PropertyHookType::class), (int) $class->isFinal(), (int) $class->isInternal(), ':';
echo implode(',', $class->getInterfaceNames()), '|';
foreach (PropertyHookType::cases() as $case) {
    echo $case->name, ':', $case->value, ':';
    echo (int) ($case === PropertyHookType::from($case->value)), '|';
}
var_dump(PropertyHookType::tryFrom('missing'));
try {
    PropertyHookType::from('missing');
} catch (ValueError $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "PropertyHookType:101:BackedEnum,UnitEnum|",
            "Get:get:1|Set:set:1|NULL\n",
            "\"missing\" is not a valid backing value for enum PropertyHookType",
        )
    );
}

#[test]
fn reflection_property_exposes_only_declared_property_hooks() {
    assert_eq!(
        run_php(
            r#"<?php
#[Attribute]
class HookMarker {
    public function __construct(public string $name) {}
}

class Hooked {
    public int $both {
        #[HookMarker('getter')]
        get => 1;
        #[HookMarker('setter')]
        set {}
    }
    public int $getter {
        #[HookMarker('only')]
        get => 2;
    }
    public int $plain;
}

foreach (['both', 'getter', 'plain'] as $name) {
    $property = new ReflectionProperty(Hooked::class, $name);
    echo $name, ':', (int) $property->hasHook(PropertyHookType::Get), (int) $property->hasHook(PropertyHookType::Set), ':';
    foreach ($property->getHooks() as $kind => $method) {
        $marker = $method->getAttributes()[0]->newInstance();
        echo $kind, '=', $method->name, ':', $marker->name, ':';
        echo count($method->getParameters()), '|';
    }
    $set = $property->getHook(PropertyHookType::Set);
    echo $set?->name ?? 'NULL', "\n";
}
"#,
        ),
        concat!(
            "both:11:get=$both::get:getter:0|set=$both::set:setter:1|",
            "$both::set\n",
            "getter:10:get=$getter::get:only:0|NULL\n",
            "plain:00:NULL\n",
        )
    );
}

#[test]
fn reflection_property_hook_methods_render_their_implicit_signatures() {
    assert_eq!(
        run_php(
            r#"<?php
class RenderedHooks {
    public string $value {
        get => $this->value;
        set => $value;
    }
}
$property = new ReflectionProperty(RenderedHooks::class, 'value');
$get = (string) $property->getHook(PropertyHookType::Get);
$set = (string) $property->getHook(PropertyHookType::Set);
echo (int) str_contains($get, 'public method $value::get'), ':';
echo (int) str_contains($get, '- Parameters [0]'), ':';
echo (int) str_contains($get, '- Return [ string ]'), '|';
echo (int) str_contains($set, 'public method $value::set'), ':';
echo (int) str_contains($set, 'Parameter #0 [ <required> string $value ]'), ':';
echo (int) str_contains($set, '- Return [ void ]'), ':';
$setMethod = $property->getHook(PropertyHookType::Set);
echo (int) $setMethod->hasReturnType(), ':', $setMethod->getReturnType()->getName();
"#,
        ),
        "1:1:1|1:1:1:1:void"
    );
}

#[test]
fn deprecated_attribute_resolves_deferred_constants_when_the_callable_is_invoked() {
    assert_eq!(
        run_php(
            r#"<?php
define('DEFERRED_DEPRECATION_MESSAGE', 'resolved later');
set_error_handler(function ($level, $message) {
    echo $message;
    return true;
});

#[Deprecated(DEFERRED_DEPRECATION_MESSAGE)]
function deferred_deprecation() {}

deferred_deprecation();
"#,
        ),
        "Function deferred_deprecation() is deprecated, resolved later"
    );
}

#[test]
fn deprecated_constants_report_direct_dynamic_and_dependency_reads() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $level, ':', $message, '|';
    return true;
});

#[Deprecated('replace prefix')]
const LEGACY_PREFIX = 'A';
const COMPOSED_VALUE = LEGACY_PREFIX . 'B';

#[Deprecated(LEGACY_PREFIX, since: '8.5')]
const LEGACY_VALUE = 7;

class DeprecatedConstantProbe {
    #[Deprecated('replace member')]
    public const LEGACY = 'X';
    public const COMPOSED = self::LEGACY . 'Y';
}

echo COMPOSED_VALUE, '|';
echo LEGACY_VALUE, '|';
echo (int) defined('LEGACY_VALUE'), '|';
echo DeprecatedConstantProbe::COMPOSED, '|';
echo constant('DeprecatedConstantProbe::LEGACY');
"#,
        ),
        concat!(
            "16384:Constant LEGACY_PREFIX is deprecated, replace prefix|AB|",
            "16384:Constant LEGACY_PREFIX is deprecated, replace prefix|",
            "16384:Constant LEGACY_VALUE is deprecated since 8.5, A|7|1|",
            "16384:Constant DeprecatedConstantProbe::LEGACY is deprecated, replace member|XY|",
            "16384:Constant DeprecatedConstantProbe::LEGACY is deprecated, replace member|X",
        )
    );
}

#[test]
fn deprecated_traits_report_at_direct_use_and_honor_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $message, '|';
    return true;
});

#[Deprecated('retire left', since: '8.5')]
trait LegacyLeft {
    public function select() { return 'left'; }
}

#[Deprecated('retire right')]
trait LegacyRight {
    public function select() { return 'right'; }
}

trait LegacyComposite {
    use LegacyLeft;
}

class DeprecatedTraitConsumer {
    use Legacyleft, LegacyRight {
        LegacyLeft::select insteadof LegacyRight;
    }
}

trait PlainConstantTrait { public const VALUE = 1; }

echo (new DeprecatedTraitConsumer())->select(), '|';
echo (int) defined('PlainConstantTrait::VALUE'), '|';
try {
    constant('PlainConstantTrait::VALUE');
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "Trait LegacyLeft used by LegacyComposite is deprecated since 8.5, retire left|",
            "Trait LegacyLeft used by DeprecatedTraitConsumer is deprecated since 8.5, retire left|",
            "Trait LegacyRight used by DeprecatedTraitConsumer is deprecated, retire right|",
            "left|0|Cannot access trait constant PlainConstantTrait::VALUE directly",
        )
    );
}

#[test]
fn deprecated_enum_case_and_runtime_class_constant_keep_their_values() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $message, '|';
    return true;
});

define('RUNTIME_SUFFIX', random_int(1, 2) === 1 ? 'a' : 'b');

enum DeprecatedCaseProbe {
    #[Deprecated('use Current')]
    case Legacy;
    case Current;
}

class DeferredConstantProbe {
    #[Deprecated]
    public const VALUE = self::class . '-' . RUNTIME_SUFFIX;
}

echo DeprecatedCaseProbe::Legacy->name, '|';
$value = DeferredConstantProbe::VALUE;
echo (int) ($value === 'DeferredConstantProbe-' . RUNTIME_SUFFIX);
"#,
        ),
        concat!(
            "Enum case DeprecatedCaseProbe::Legacy is deprecated, use Current|Legacy|",
            "Constant DeferredConstantProbe::VALUE is deprecated|1",
        )
    );
}

#[test]
fn deprecated_attribute_rejects_non_trait_class_like_targets_during_compilation() {
    for (kind, declaration) in [
        ("class", "class ForbiddenDeprecatedClass {}"),
        ("interface", "interface ForbiddenDeprecatedInterface {}"),
        ("enum", "enum ForbiddenDeprecatedEnum {}"),
    ] {
        let source = format!("<?php #[Deprecated] {declaration}");
        let error = run_php_expect_error(&source).to_string();
        assert!(
            error.contains(&format!("Cannot apply #[\\Deprecated] to {kind}")),
            "unexpected {kind} diagnostic: {error}"
        );
    }
}

#[test]
fn reflection_attribute_exposes_only_its_public_name_projection() {
    assert_eq!(
        run_php(
            "<?php #[Attribute] class VisibleLabel {} #[VisibleLabel] class VisibleSubject {} $attribute = (new ReflectionClass(VisibleSubject::class))->getAttributes()[0]; $properties = get_object_vars($attribute); echo count($properties), ':', $properties['name'], ':', $attribute->getName();"
        ),
        "1:VisibleLabel:VisibleLabel"
    );
}

#[test]
fn class_scoped_attribute_constants_are_available_before_member_compilation() {
    assert_eq!(
        run_php(
            "<?php #[Attribute] class ScopedLabel { public function __construct(public string $owner, public int $value) {} } #[ScopedLabel(self::class, self::VALUE)] class ScopedSubject { private const VALUE = 42; #[ScopedLabel(self::class, self::VALUE)] public function read(#[ScopedLabel(self::class, self::VALUE)] $input): void {} } $class = new ReflectionClass(ScopedSubject::class); foreach ([$class->getAttributes()[0], $class->getMethod('read')->getAttributes()[0], $class->getMethod('read')->getParameters()[0]->getAttributes()[0]] as $attribute) { echo implode(':', $attribute->getArguments()), '|'; }"
        ),
        "ScopedSubject:42|ScopedSubject:42|ScopedSubject:42|"
    );
}

#[test]
fn reflection_attributes_evaluate_runtime_constants_and_missing_classes_on_demand() {
    assert_eq!(
        run_php(
            "<?php define('RUNTIME_ATTRIBUTE_VALUE', 'ready'); #[Attribute] class DeferredLabel { public function __construct(public mixed $value) {} } #[DeferredLabel([RUNTIME_ATTRIBUTE_VALUE => RUNTIME_ATTRIBUTE_VALUE])] class DeferredSubject {} $arguments = (new ReflectionClass(DeferredSubject::class))->getAttributes()[0]->getArguments(); echo array_key_first($arguments[0]), ':', $arguments[0][RUNTIME_ATTRIBUTE_VALUE], '|'; #[DeferredLabel(MissingArgumentClass::VALUE)] class MissingArgumentSubject {} $missing = (new ReflectionClass(MissingArgumentSubject::class))->getAttributes()[0]; try { $missing->getArguments(); } catch (Error $error) { echo $error->getMessage(), '|'; } try { $missing->newInstance(); } catch (Error $error) { echo $error->getMessage(), '|'; } #[Attribute(MissingMarkerClass::FLAGS)] class InvalidDeferredMarker {} #[InvalidDeferredMarker] class InvalidDeferredTarget {} try { (new ReflectionClass(InvalidDeferredTarget::class))->getAttributes()[0]->newInstance(); } catch (Error $error) { echo $error->getMessage(); }"
        ),
        "ready:ready|Class \"MissingArgumentClass\" not found|Class \"MissingArgumentClass\" not found|Class \"MissingMarkerClass\" not found"
    );
}

#[test]
fn attribute_marker_runtime_constants_use_the_marker_declaration_scope() {
    assert_eq!(
        run_php(
            "<?php #[Attribute(parent::MASK)] class DeferredScopedMarker extends DeferredMarkerBase {} class DeferredMarkerBase { protected const MASK = Attribute::TARGET_CLASS; } #[DeferredScopedMarker] class DeferredScopedTarget {} echo get_class((new ReflectionClass(DeferredScopedTarget::class))->getAttributes()[0]->newInstance());"
        ),
        "DeferredScopedMarker"
    );
}

#[test]
fn reflected_trait_method_attributes_use_the_consumer_scope() {
    assert_eq!(
        run_php(
            "<?php trait DeferredAttributeTrait { #[DeferredTraitLabel(self::class, self::VALUE)] public function tagged() {} } class DeferredTraitConsumer { use DeferredAttributeTrait; private const VALUE = 'consumer'; } $arguments = (new ReflectionClass(DeferredTraitConsumer::class))->getMethod('tagged')->getAttributes()[0]->getArguments(); echo implode(':', $arguments), '|'; try { (new ReflectionClass(DeferredAttributeTrait::class))->getMethod('tagged')->getAttributes()[0]->getArguments(); } catch (Error $error) { echo $error->getMessage(); }"
        ),
        "DeferredTraitConsumer:consumer|Undefined constant self::VALUE"
    );
}

#[test]
fn reflected_closure_attributes_follow_bound_called_scope() {
    assert_eq!(
        run_php(
            "<?php class DeferredClosureFirst { private const VALUE = 'first'; public static function make() { return #[DeferredClosureLabel(self::class, self::VALUE)] function (#[DeferredClosureLabel(self::class, self::VALUE)] $value) {}; } } class DeferredClosureSecond { private const VALUE = 'second'; } $reflection = new ReflectionFunction(DeferredClosureFirst::make()->bindTo(null, DeferredClosureSecond::class)); echo implode(':', $reflection->getAttributes()[0]->getArguments()), '|', implode(':', $reflection->getParameters()[0]->getAttributes()[0]->getArguments());"
        ),
        "DeferredClosureSecond:second|DeferredClosureSecond:second"
    );
}

#[test]
fn reflection_parameter_stringification_is_available_to_framework_introspection() {
    assert_eq!(
        run_php(
            "<?php function reflected_parameter_string($required, &...$rest) {} foreach ((new ReflectionFunction('reflected_parameter_string'))->getParameters() as $parameter) { echo (string) $parameter, '|'; }"
        ),
        "Parameter #0 [ <required> $required ]|Parameter #1 [ <optional> &...$rest ]|"
    );
}

#[test]
fn anonymous_class_attribute_scope_matches_its_public_reflection_name() {
    assert_eq!(
        run_php(
            "<?php $reflection = new ReflectionObject(new #[DeferredAnonymousLabel(self::class, self::VALUE)] class() { private const VALUE = 'anonymous'; }); $arguments = $reflection->getAttributes()[0]->getArguments(); echo (int) ($arguments[0] === $reflection->getName()), ':', $arguments[1];"
        ),
        "1:anonymous"
    );
}

#[test]
fn test_get_class_returns_class_name() {
    let out = run_php(
        r#"<?php
class Foo {}
$obj = new Foo();
echo get_class($obj);
"#,
    );
    assert_eq!(out, "Foo");
}

#[test]
fn test_get_class_with_non_object_throws_type_error() {
    let out = run_php(
        r#"<?php
try {
    get_class("hello");
} catch (TypeError $error) {
    echo $error->getMessage();
}
"#,
    );
    assert_eq!(
        out,
        "get_class(): Argument #1 ($object) must be of type object, string given"
    );
}

#[test]
fn get_class_without_argument_deprecates_before_returning_php_85_lexical_scope() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class BaseName {
    public static function direct() { echo get_class(), "\n"; }
    public function instance() { echo get_class(), "\n"; }
}
class ChildName extends BaseName {}
set_error_handler(function($level, $message) { throw new Exception($message); });
try {
    ChildName::direct();
} catch (Exception $error) {
    echo 'caught:', $error->getMessage(), "\n";
}
restore_error_handler();
ChildName::direct();
(new ChildName())->instance();
try {
    get_class();
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
            "/virtual/get-class.php",
            "/virtual",
        ),
        concat!(
            "caught:Calling get_class() without arguments is deprecated\n",
            "\nDeprecated: Calling get_class() without arguments is deprecated in /virtual/get-class.php on line 3\n",
            "BaseName\n",
            "\nDeprecated: Calling get_class() without arguments is deprecated in /virtual/get-class.php on line 4\n",
            "BaseName\n",
            "get_class() without arguments must be called from within a class",
        )
    );
}

#[test]
fn reflection_class_creates_an_instance_without_running_its_constructor() {
    let out = run_php(
        r#"<?php
class ConstructorProbe {
    public int $value = 7;
    public function __construct() { $this->value = 99; }
}
$object = (new ReflectionClass(ConstructorProbe::class))->newInstanceWithoutConstructor();
echo get_class($object) . ':' . $object->value;
"#,
    );
    assert_eq!(out, "ConstructorProbe:7");
}

#[test]
fn reflection_class_distinguishes_user_and_internal_classes() {
    let out = run_php(
        r#"<?php
class UserDefinedReflectionProbe {}
echo (new ReflectionClass(UserDefinedReflectionProbe::class))->isInternal() ? 'bad' : 'user';
echo ':';
echo (new ReflectionClass(stdClass::class))->isInternal() ? 'internal' : 'bad';
echo ':';
echo (new ReflectionClass(UserDefinedReflectionProbe::class))->isUserDefined() ? 'defined' : 'bad';
echo ':';
echo (new ReflectionClass(stdClass::class))->isUserDefined() ? 'bad' : 'builtin';
"#,
    );
    assert_eq!(out, "user:internal:defined:builtin");
}

#[test]
fn reflection_class_lists_property_metadata_and_filters_private_properties() {
    let out = run_php(
        r#"<?php
class ReflectedPropertyParent { private int $hidden = 1; }
class ReflectedPropertyChild extends ReflectedPropertyParent {
    public static string $shared = 'x';
    protected readonly int $locked;
}
$properties = (new ReflectionClass(ReflectedPropertyChild::class))->getProperties();
foreach ($properties as $property) {
    echo $property->name . ':' . $property->class . ':' . $property->getModifiers() . ':';
    echo ($property->isStatic() ? 's' : '-') . ($property->isReadOnly() ? 'r' : '-') . '|';
}
echo count((new ReflectionClass(ReflectedPropertyParent::class))->getProperties(ReflectionProperty::IS_PRIVATE));
"#,
    );
    assert_eq!(
        out,
        "shared:ReflectedPropertyChild:17:s-|locked:ReflectedPropertyChild:130:-r|1"
    );
}

#[test]
fn reflection_class_get_property_hides_inherited_private_properties() {
    let out = run_php(
        r#"<?php
class ReflectedPropertyLookupParent {
    private int $hidden;
    private static int $hiddenStatic;
    protected int $inherited;
}
class ReflectedPropertyLookupChild extends ReflectedPropertyLookupParent {}

$child = new ReflectionClass(ReflectedPropertyLookupChild::class);
foreach (['hidden', 'hiddenStatic', 'inherited'] as $name) {
    try {
        $property = $child->getProperty($name);
        echo "$name:{$property->class}\n";
    } catch (ReflectionException $exception) {
        echo $exception->getMessage(), "\n";
    }
}
echo (new ReflectionClass(ReflectedPropertyLookupParent::class))->getProperty('hidden')->class;
"#,
    );
    assert_eq!(
        out,
        concat!(
            "Property ReflectedPropertyLookupChild::$hidden does not exist\n",
            "Property ReflectedPropertyLookupChild::$hiddenStatic does not exist\n",
            "inherited:ReflectedPropertyLookupParent\n",
            "ReflectedPropertyLookupParent",
        )
    );
}

#[test]
fn reflection_property_distinguishes_declared_and_promoted_defaults() {
    let out = run_php(
        r#"<?php
class ReflectedDefaults {
    public $implicit;
    public int $uninitialized;
    public $explicit = 3;
    public function __construct(public $promoted = 4 { get => $this->promoted; }) {}
}
foreach (['implicit', 'uninitialized', 'explicit', 'promoted'] as $name) {
    $property = new ReflectionProperty(ReflectedDefaults::class, $name);
    echo $name, ':', (int) $property->hasDefaultValue(), ':';
    if ($property->hasDefaultValue()) {
        var_dump($property->getDefaultValue());
    } else {
        echo "none\n";
    }
}
"#,
    );
    assert_eq!(
        out,
        concat!(
            "implicit:1:NULL\n",
            "uninitialized:0:none\n",
            "explicit:1:int(3)\n",
            "promoted:0:none\n",
        )
    );
}

#[test]
fn reflection_property_reports_final_abstract_and_virtual_hook_flags() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class ReflectedHookFlags {
    abstract public $abstract { get; }
    final public $backed { get => $this->backed; }
    public $virtual { get => 42; }
}
foreach ((new ReflectionClass(ReflectedHookFlags::class))->getProperties() as $property) {
    echo $property->name, ':', (int) $property->isFinal(),
        (int) $property->isAbstract(), (int) $property->isVirtual(),
        ':', $property->getModifiers(), '|';
}
"#,
        ),
        "abstract:011:577|backed:100:33|virtual:001:513|"
    );
}

#[test]
fn reflection_property_stringifies_php_85_declaration_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class ReflectedPropertyStrings {
    abstract protected $abstract { get; }
    final public $backed { get => $this->backed; set => $value; }
    public protected(set) readonly int $locked;
    public static string $shared = 'x';
    public $implicit;
    public int $uninitialized;
    public $virtual { get => 42; set {} }
}
foreach (['abstract', 'backed', 'locked', 'shared', 'implicit', 'uninitialized', 'virtual'] as $name) {
    echo new ReflectionProperty(ReflectedPropertyStrings::class, $name);
}
"#,
        ),
        concat!(
            "Property [ abstract protected virtual $abstract { get; } ]\n",
            "Property [ final public $backed = NULL { get; set; } ]\n",
            "Property [ public protected(set) readonly int $locked ]\n",
            "Property [ public static string $shared = 'x' ]\n",
            "Property [ public $implicit = NULL ]\n",
            "Property [ public int $uninitialized ]\n",
            "Property [ public virtual $virtual { get; set; } ]\n",
        )
    );
}

#[test]
fn reflection_object_stringification_preserves_uninitialized_lazy_state() {
    assert_eq!(
        run_php(
            r#"<?php
class ReflectedLazyString {
    public int $value;
    public function initialize(): void { $this->value = 1; }
}
$reflection = new ReflectionClass(ReflectedLazyString::class);
$objects = [
    $reflection->newLazyGhost(function ($object) { echo "ghost initialized\n"; }),
    $reflection->newLazyProxy(function ($object) { echo "proxy initialized\n"; return new ReflectedLazyString(); }),
];
foreach ($objects as $object) {
    $rendered = (new ReflectionObject($object))->__toString();
    echo (int) str_contains($rendered, 'Object of class [ <user> class ReflectedLazyString ]'), ':';
    echo (int) str_contains($rendered, 'Property [ public int $value ]'), ':';
    echo (int) $reflection->isUninitializedLazyObject($object), "\n";
}
"#,
        ),
        "1:1:1\n1:1:1\n"
    );
}

#[test]
fn reflection_properties_keep_child_first_source_declaration_order() {
    assert_eq!(
        run_php(
            r#"<?php
class ReflectedOrderedParent {
    public $first;
    public static $shared;
    protected $last;
}
class ReflectedOrderedChild extends ReflectedOrderedParent {
    public $child;
}
foreach ((new ReflectionClass(ReflectedOrderedChild::class))->getProperties() as $property) {
    echo $property->getName(), ':';
}
try {
    $object = new ReflectedOrderedChild();
    echo $object->last;
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        "child:first:shared:last:Cannot access protected property ReflectedOrderedChild::$last"
    );
}

#[test]
fn reflection_class_lists_direct_extended_and_inherited_interface_names() {
    let out = run_php(
        r#"<?php
interface RootInterface {}
interface ChildInterface extends RootInterface {}
interface ParentInterface {}
class ReflectedInterfaceParent implements ParentInterface {}
class ReflectedInterfaceChild extends ReflectedInterfaceParent implements ChildInterface {}
echo implode(',', (new ReflectionClass(ReflectedInterfaceChild::class))->getInterfaceNames());
echo '|';
echo implode(',', (new ReflectionClass(ChildInterface::class))->getInterfaceNames());
"#,
    );
    assert_eq!(
        out,
        "ParentInterface,ChildInterface,RootInterface|RootInterface"
    );
}

#[test]
fn reflection_class_lists_only_directly_used_trait_names() {
    let out = run_php(
        r#"<?php
trait ReflectedRootTrait {}
trait ReflectedNestedTrait { use ReflectedRootTrait; }
trait ReflectedParentTrait {}
class ReflectedTraitParent { use ReflectedParentTrait; }
class ReflectedTraitChild extends ReflectedTraitParent { use ReflectedNestedTrait; }
echo implode(',', (new ReflectionClass(ReflectedTraitParent::class))->getTraitNames());
echo '|';
echo implode(',', (new ReflectionClass(ReflectedTraitChild::class))->getTraitNames());
echo '|';
echo implode(',', (new ReflectionClass(ReflectedNestedTrait::class))->getTraitNames());
"#,
    );
    assert_eq!(
        out,
        "ReflectedParentTrait|ReflectedNestedTrait|ReflectedRootTrait"
    );
}

#[test]
fn reflection_class_lists_and_filters_constant_values() {
    let out = run_php(
        r#"<?php
class ReflectedConstantParent {
    public const PUB = 1;
    protected const PRO = 2;
    private const HIDDEN = 3;
    final public const FIN = 4;
}
class ReflectedConstantChild extends ReflectedConstantParent {
    private const OWN = 5;
}
$reflection = new ReflectionClass(ReflectedConstantChild::class);
foreach ($reflection->getConstants() as $name => $value) {
    echo $name . '=' . $value . ',';
}
echo '|';
foreach ($reflection->getConstants(4) as $name => $value) {
    echo $name . '=' . $value . ',';
}
echo '|';
foreach ($reflection->getConstants(32) as $name => $value) {
    echo $name . '=' . $value . ',';
}
"#,
    );
    assert_eq!(out, "OWN=5,PUB=1,PRO=2,FIN=4,|OWN=5,|FIN=4,");
}

#[test]
fn reflection_class_exposes_constant_objects_and_default_properties() {
    let out = run_php(
        r#"<?php
class ReflectedDefaults {
    public const PUBLIC_VALUE = 3;
    protected static string $label = 'ready';
    public int $count = 2;
    public string $uninitialized;
}
$reflection = new ReflectionClass(ReflectedDefaults::class);
$constant = $reflection->getReflectionConstants()[0];
echo $constant->name, ':', count($constant->getAttributes()), '|';
foreach ($reflection->getDefaultProperties() as $name => $value) { echo $name, '=', $value, ','; }
echo '|';
foreach ($reflection->getProperties() as $property) {
    echo $property->name, ':', (int) $property->isDefault(), (int) $property->isPublic(), (int) $property->isProtected(), (int) $property->isStatic(), ',';
}
"#,
    );
    assert_eq!(
        out,
        "PUBLIC_VALUE:0|label=ready,count=2,|label:1011,count:1100,uninitialized:1100,"
    );
}

#[test]
fn declared_class_like_inventories_report_canonical_kinds_and_class_aliases() {
    let out = run_php(
        r#"<?php
class DeclaredInventoryClass {}
interface DeclaredInventoryInterface {}
trait DeclaredInventoryTrait {}
enum DeclaredInventoryEnum {}
class_alias(DeclaredInventoryClass::class, 'DeclaredInventoryAlias');
echo in_array(DeclaredInventoryClass::class, get_declared_classes(), true) ? 'c' : '-';
echo in_array(DeclaredInventoryEnum::class, get_declared_classes(), true) ? 'e' : '-';
echo in_array('declaredinventoryalias', get_declared_classes(), true) ? 'a' : '-';
echo in_array(DeclaredInventoryInterface::class, get_declared_interfaces(), true) ? 'i' : '-';
echo in_array(DeclaredInventoryTrait::class, get_declared_traits(), true) ? 't' : '-';
"#,
    );
    assert_eq!(out, "ceait");
}

#[test]
fn reflection_functions_and_methods_report_parameter_counts_and_metadata() {
    let out = run_php(
        r#"<?php
function &reflectedCount(&$required, $optional = 1, ...$rest) {}
class ReflectedCountParent {
    public function &counted(string $required, ?int $optional = null): void {}
}
class ReflectedCountChild extends ReflectedCountParent {}
$function = new ReflectionFunction('reflectedCount');
echo $function->getNumberOfParameters(), ':', $function->getNumberOfRequiredParameters(), ':', (int) $function->returnsReference(), (int) $function->isClosure(), (int) $function->hasReturnType(), ':';
$functionParameters = $function->getParameters();
echo count($functionParameters), ':', (int) $functionParameters[0]->isOptional(), (int) $functionParameters[1]->isOptional(), (int) $functionParameters[2]->isOptional(), ':';
echo (int) $functionParameters[0]->isPassedByReference(), (int) $functionParameters[1]->isPassedByReference(), '|';
$method = new ReflectionMethod(new ReflectedCountChild(), 'counted');
echo $method->getNumberOfParameters(), ':', $method->getNumberOfRequiredParameters(), ':', (int) $method->returnsReference(), (int) $method->isClosure(), (int) $method->hasReturnType(), (int) $method->hasTentativeReturnType(), ':', $method->getReturnType()->getName(), ':';
$parameters = $method->getParameters();
echo count($parameters), ':', $parameters[0]->getName(), ':', $parameters[1]->isDefaultValueAvailable(), ':';
echo (int) $parameters[0]->hasType(), (int) $functionParameters[0]->hasType();
"#,
    );
    assert_eq!(out, "3:1:100:3:011:10|2:1:1010:void:2:required:1:10");
}

#[test]
fn reflection_method_get_closure_binds_instance_and_late_static_scope() {
    let out = run_php(
        r#"<?php
class ReflectedClosureParent {
    protected function joined($first, $second = 'b') {
        return static::class . ':' . $first . ':' . $second;
    }
    public static function scoped($value) {
        return static::class . ':' . $value;
    }
}

class ReflectedClosureChild extends ReflectedClosureParent {}
$object = new ReflectedClosureChild();
$instance = (new ReflectionMethod($object, 'joined'))->getClosure($object);
echo $instance('a'), '|';
$static = (new ReflectionMethod(ReflectedClosureChild::class, 'scoped'))->getClosure();
echo $static('x');
"#,
    );
    assert_eq!(out, "ReflectedClosureChild:a:b|ReflectedClosureChild:x");
}

#[test]
fn reflection_function_get_closure_preserves_identity_and_function_state() {
    let out = run_php(
        r#"<?php
function reflectedStaticState() {
    static $values = [];
    $values[] = count($values);
    return implode(',', $values);
}

$first = new ReflectionFunction('reflectedStaticState');
$second = new ReflectionFunction('reflectedStaticState');
echo $first->getClosure()(), '|';
echo $second->getClosure()(), '|';
echo reflectedStaticState(), '|';
echo (new ReflectionFunction('strlen'))->getClosure()('abcd'), '|';

$captured = 'kept';
$closure = function () use ($captured) { return $captured; };
$reflected = (new ReflectionFunction($closure))->getClosure();
echo ($reflected === $closure ? 'same:' : 'copy:'), $reflected();
"#,
    );
    assert_eq!(out, "0|0,1|0,1,2|4|same:kept");
}

#[test]
fn reflected_method_closure_keeps_nested_captured_arguments_aligned() {
    let out = run_php(
        r#"<?php
class NestedContainer { public string $marker = 'container'; }
class NestedLoader { public string $marker = 'loader'; }
class NestedConfigurator {
    public function configure(NestedContainer $container, NestedLoader $loader): string {
        return $container->marker . ':' . $loader->marker;
    }
}

class NestedInvoker {
    public function invoke(Closure $callback): string {
        return $callback(new NestedContainer(), 'environment');
    }
}

$configurator = new NestedConfigurator();
$loader = new NestedLoader();
$callback = function (NestedContainer $container) use ($configurator, $loader): string {
    $method = new ReflectionMethod($configurator, 'configure');
    return $method->getClosure($configurator)($container, $loader);
};
echo (new NestedInvoker())->invoke($callback);
"#,
    );
    assert_eq!(out, "container:loader");
}

#[test]
fn reflection_class_get_methods_reports_inheritance_filters_and_metadata() {
    let out = run_php(
        r#"<?php
class MethodInventoryParent {
    protected function inherited($required, $optional = 1) {}
    private function hidden() {}
}
class MethodInventoryChild extends MethodInventoryParent {
    public static final function visible() {}
    public function __construct() {}
}
$reflection = new ReflectionClass(MethodInventoryChild::class);
$all = $reflection->getMethods();
foreach ($all as $method) {
    echo $method->getName(), ':', $method->getDeclaringClass()->name, ':', $method->getModifiers(), ':';
    echo $method->isConstructor() ? 'c' : '-', '|';
}
echo '#';
foreach ($reflection->getMethods(ReflectionMethod::IS_PUBLIC) as $method) {
    echo $method->getName(), ',';
}
"#,
    );
    assert_eq!(
        out,
        "visible:MethodInventoryChild:49:-|__construct:MethodInventoryChild:1:c|inherited:MethodInventoryParent:2:-|hidden:MethodInventoryParent:4:-|#visible,__construct,"
    );
}

#[test]
fn test_class_exists_true() {
    let out = run_php(
        r#"<?php
class MyClass {}
echo class_exists('MyClass') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_class_exists_false() {
    let out = run_php(
        r#"<?php
echo class_exists('NonExistent') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "no");
}

#[test]
fn test_method_exists_with_object_true() {
    let out = run_php(
        r#"<?php
class Bar {
    public function hello() {}
}
$obj = new Bar();
echo method_exists($obj, 'hello') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_with_object_false() {
    let out = run_php(
        r#"<?php
class Baz {
    public function hello() {}
}
$obj = new Baz();
echo method_exists($obj, 'nonexistent') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "no");
}

#[test]
fn test_method_exists_with_string_class_name() {
    let out = run_php(
        r#"<?php
class Qux {
    public function doStuff() {}
}
echo method_exists('Qux', 'doStuff') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_with_string_class_name_false() {
    let out = run_php(
        r#"<?php
class Corge {
    public function doStuff() {}
}
echo method_exists('Corge', 'missing') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "no");
}

// -- method_exists with inheritance --

#[test]
fn test_method_exists_inherited_method() {
    let out = run_php(
        r#"<?php
class A {
    public function foo() {}
}
class B extends A {}
echo method_exists('B', 'foo') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_inherited_on_object() {
    let out = run_php(
        r#"<?php
class Parent1 {
    public function parentMethod() {}
}
class Child1 extends Parent1 {}
$c = new Child1();
echo method_exists($c, 'parentMethod') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_deep_inheritance() {
    let out = run_php(
        r#"<?php
class GrandParent1 {
    public function deep() {}
}
class Parent2 extends GrandParent1 {}
class Child2 extends Parent2 {}
echo method_exists('Child2', 'deep') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

// -- class_exists excludes interfaces and traits --

#[test]
fn test_class_exists_interface_false() {
    let out = run_php(
        r#"<?php
interface MyInterface {}
echo class_exists('MyInterface') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "no");
}

#[test]
fn test_class_exists_trait_false() {
    let out = run_php(
        r#"<?php
trait MyTrait {}
echo class_exists('MyTrait') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "no");
}

#[test]
fn test_class_exists_real_class_still_true() {
    let out = run_php(
        r#"<?php
interface I {}
trait T {}
class C implements I { use T; }
echo class_exists('C') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

// -- method_exists with traits --

#[test]
fn test_method_exists_trait_method() {
    let out = run_php(
        r#"<?php
trait Greetable {
    public function greet() { return "hi"; }
}
class Hello {
    use Greetable;
}
echo method_exists('Hello', 'greet') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_trait_method_on_object() {
    let out = run_php(
        r#"<?php
trait Greetable {
    public function greet() { return "hi"; }
}
class Hello {
    use Greetable;
}
$h = new Hello();
echo method_exists($h, 'greet') ? 'yes' : 'no';
"#,
    );
    assert_eq!(out, "yes");
}
