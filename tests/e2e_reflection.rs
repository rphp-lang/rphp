mod common;
use common::run_php;

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
fn test_get_class_with_non_object_returns_false() {
    let out = run_php(
        r#"<?php
$result = get_class("hello");
var_dump($result);
"#,
    );
    assert_eq!(out, "bool(false)\n");
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
"#,
    );
    assert_eq!(out, "user:internal");
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
