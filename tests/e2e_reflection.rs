mod common;
use common::run_php;

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
fn get_class_without_argument_uses_php_82_lexical_scope_without_deprecation() {
    assert_eq!(
        run_php(
            r#"<?php
class BaseName {
    public static function direct() { echo get_class(), "\n"; }
    public function instance() { echo get_class(), "\n"; }
}
class ChildName extends BaseName {}
ChildName::direct();
(new ChildName())->instance();
try {
    get_class();
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "BaseName\n",
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
        "locked:ReflectedPropertyChild:130:-r|shared:ReflectedPropertyChild:17:s-|1"
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
        "PUBLIC_VALUE:0|label=ready,count=2,|count:1100,uninitialized:1100,label:1011,"
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
