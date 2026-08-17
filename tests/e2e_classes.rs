/// Tests for classes and objects (basic)
mod common;
use common::{run_php, run_php_expect_error, run_php_with_source_context};

#[test]
fn anonymous_classes_compose_traits_and_hide_internal_identity() {
    assert_eq!(
        run_php(
            r#"<?php
trait SharedGreeting {
    public function greeting() { return "hello"; }
    public function original() { return "original"; }
    public function callRenamed() { return $this->renamed(); }
}
$object = new class {
    use SharedGreeting { original as private renamed; }
    private $value;
};
echo $object->greeting(), ":", $object->callRenamed(), "\n";
var_dump($object);
"#,
        ),
        "hello:original\nobject(class@anonymous)#1 (1) {\n  [\"value\":\"class@anonymous\":private]=>\n  NULL\n}\n"
    );
}

#[test]
fn anonymous_class_rejects_an_explicit_abstract_method_at_its_declaration() {
    let error =
        run_php_expect_error("<?php $object = new class { abstract public function missing(); }");
    assert!(
        error
            .to_string()
            .contains("Anonymous class method missing() must not be abstract"),
        "unexpected error: {error}"
    );
}

#[test]
fn readonly_classes_reject_every_dynamic_creation_path_but_allow_magic_set() {
    assert_eq!(
        run_php(
            r#"<?php
readonly class Sealed {}
$sealed = new Sealed();
foreach (['direct', 'compound', 'increment', 'reference'] as $mode) {
    try {
        if ($mode === 'direct') $sealed->direct = 1;
        elseif ($mode === 'compound') $sealed->compound += 1;
        elseif ($mode === 'increment') $sealed->increment++;
        else $reference =& $sealed->reference;
    } catch (Error $error) {
        echo $mode, ':', $error->getMessage(), "\n";
    }
}
readonly class Overloaded {
    public function __set($name, $value) { echo "set:$name:$value\n"; }
}
$overloaded = new Overloaded();
$overloaded->accepted = 7;
"#,
        ),
        "direct:Cannot create dynamic property Sealed::$direct\ncompound:Cannot create dynamic property Sealed::$compound\nincrement:Cannot create dynamic property Sealed::$increment\nreference:Cannot create dynamic property Sealed::$reference\nset:accepted:7\n"
    );
}

#[test]
fn anonymous_readonly_classes_preserve_semantics_and_modifier_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
$readonly = new readonly class {
    public int $value;
    public function __construct() { $this->value = 2; }
};
var_dump($readonly->value);
try { $readonly->value = 3; } catch (Error $error) { echo $error->getMessage(), "\n"; }
$dynamic = new #[AllowDynamicProperties] class {};
$dynamic->value = 4;
var_dump($dynamic->value);
"#,
        ),
        "int(2)\nCannot modify readonly property class@anonymous::$value\nint(4)\n"
    );

    for (source, expected) in [
        (
            "<?php new #[AllowDynamicProperties] readonly class {};",
            "Cannot apply #[\\AllowDynamicProperties] to readonly class class@anonymous",
        ),
        (
            "<?php new abstract class {};",
            "Cannot use the abstract modifier on an anonymous class",
        ),
        (
            "<?php new final class {};",
            "Cannot use the final modifier on an anonymous class",
        ),
        (
            "<?php new readonly readonly class {};",
            "Multiple readonly modifiers are not allowed",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected error for {source}: {error}"
        );
    }
}

#[test]
fn allow_dynamic_properties_rejects_non_dynamic_class_targets() {
    for (source, expected) in [
        (
            "<?php #[AllowDynamicProperties] interface Contract {}",
            "Cannot apply #[\\AllowDynamicProperties] to interface Contract",
        ),
        (
            "<?php #[AllowDynamicProperties] trait SharedBehavior {}",
            "Cannot apply #[\\AllowDynamicProperties] to trait SharedBehavior",
        ),
        (
            "<?php #[AllowDynamicProperties] enum Choice {}",
            "Cannot apply #[\\AllowDynamicProperties] to enum Choice",
        ),
        (
            "<?php #[\\AllowDynamicProperties] readonly class Immutable {}",
            "Cannot apply #[\\AllowDynamicProperties] to readonly class Immutable",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected error for {source}: {error}"
        );
    }
}

#[test]
fn dynamic_property_deprecation_matches_php_85_creation_exceptions() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class PlainDynamic {}
$plain = new PlainDynamic();
$plain->value = 1;
$plain->value = 2;

#[\AllowDynamicProperties]
class AllowedDynamic {}
class InheritedDynamic extends AllowedDynamic {}
$allowed = new InheritedDynamic();
$allowed->value = 1;

$standard = new stdClass();
$standard->value = 1;

class MagicDynamic {
    public function __set($name, $value) { echo "set:$name\n"; }
}
$magic = new MagicDynamic();
$magic->value = 1;
"#,
            "/virtual/dynamic-properties.php",
            "/virtual",
        ),
        "\nDeprecated: Creation of dynamic property PlainDynamic::$value is deprecated in /virtual/dynamic-properties.php on line 4\nset:value\n"
    );
}

#[test]
fn asymmetric_property_visibility_separates_read_and_write_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public private(set) int $privateValue;
    public protected(set) array $protectedValues = [];
    public function __construct(int $value) { $this->privateValue = $value; }
    public function replace(int $value) { $this->privateValue = $value; }
}
class ChildBox extends Box {
    public function append(int $value) { $this->protectedValues[] = $value; }
    public function replaceFromChild() { $this->privateValue = 99; }
}
$box = new ChildBox(1);
var_dump($box->privateValue);
try { $box->privateValue = 2; } catch (Error $error) { echo $error->getMessage(), "\n"; }
$box->replace(3);
$box->append(4);
var_dump($box->privateValue, $box->protectedValues);
try { $box->replaceFromChild(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { $box->protectedValues[] = 5; } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "int(1)\nCannot modify private(set) property Box::$privateValue from global scope\nint(3)\narray(1) {\n  [0]=>\n  int(4)\n}\nCannot modify private(set) property Box::$privateValue from scope ChildBox\nCannot indirectly modify protected(set) property Box::$protectedValues from global scope\n"
    );
}

#[test]
fn loose_object_equality_compares_class_and_nested_property_state() {
    assert_eq!(
        run_php(
            "<?php class ComparableState { private array $values = []; public function add(int $value): void { $this->values[] = $value; } } $left = new ComparableState(); $right = new ComparableState(); echo $left == $right ? 'equal:' : 'bad:'; $right->add(1); echo $left != $right ? 'different:' : 'bad:'; $left->add(1); echo $left == $right ? 'equal' : 'bad';"
        ),
        "equal:different:equal"
    );
}

#[test]
fn recursive_compound_comparisons_throw_without_losing_self_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$recursive = [&$recursive];
try { $recursive === [[]]; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { [[]] === $recursive; } catch (Error $error) { echo $error->getMessage(), "\n"; }
var_dump($recursive === $recursive);

#[AllowDynamicProperties]
class CyclicComparisonNode {}
$left = new CyclicComparisonNode();
$right = new CyclicComparisonNode();
$left->next = $left;
$right->next = $right;
try { $left == $right; } catch (Error $error) { echo $error->getMessage(), "\n"; }
var_dump($left == $left);
"#,
        ),
        "Nesting level too deep - recursive dependency?\nNesting level too deep - recursive dependency?\nbool(true)\nNesting level too deep - recursive dependency?\nbool(true)\n"
    );
}

#[test]
fn user_destructor_runs_when_a_function_releases_its_last_object_handles() {
    let out = run_php(
        r#"<?php
class DestructibleBuilder {
    public function chain() { return $this; }
    public function __destruct() { echo 'D'; }
}
class DestructibleFactory {
    public function create() {
        $builder = new DestructibleBuilder();
        return $builder->chain();
    }
}
function releaseBuilder() {
    (new DestructibleFactory())->create()->chain();
    echo 'B';
}
releaseBuilder();
echo 'A';
"#,
    );
    assert_eq!(out, "DBA");
}

#[test]
fn destructors_are_not_suppressed_when_allocator_addresses_are_reused() {
    assert_eq!(
        run_php(
            "<?php class ReusedDestructorAddress { public function __construct(private int $id) {} public function chain(): static { return $this; } public function __destruct() { echo $this->id, ','; } } function releaseSequentially(int $id): void { (new ReusedDestructorAddress($id))->chain(); } for ($id = 0; $id < 16; ++$id) releaseSequentially($id);"
        ),
        "0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,"
    );
}

#[test]
fn destructor_order_is_scope_deterministic_and_revisits_released_dependencies() {
    assert_eq!(
        run_php(
            r#"<?php
class OrderedDestructor {
    public function __construct(private string $name) {}
    public function __destruct() { echo $this->name; }
}
function releaseLocals() {
    $first = new OrderedDestructor('a');
    $second = new OrderedDestructor('b');
    $third = new OrderedDestructor('c');
}
releaseLocals();
echo '|';

class NestedDestructor {
    public function __construct(private object $child) {}
    public function __destruct() { echo 'N'; unset($this->child); }
}
class ChildDestructor {
    public function __destruct() { echo 'C'; }
}
$child = new ChildDestructor();
$parent = new NestedDestructor($child);
$rootFirst = new OrderedDestructor('1');
$rootLast = new OrderedDestructor('2');
"#,
        ),
        "abc|21NC"
    );
}

#[test]
fn reflection_object_exposes_the_declaring_source_file() {
    let file = "/tmp/rphp-reflection-object-source.php";
    assert_eq!(
        run_php_with_source_context(
            "<?php class ReflectedSource {} echo (new ReflectionObject(new ReflectedSource()))->getFileName();",
            file,
            "/tmp",
        ),
        file
    );
}

#[test]
fn instanceof_accepts_a_runtime_class_name() {
    assert_eq!(
        run_php(
            "<?php class RuntimeType {} $class = RuntimeType::class; $value = new RuntimeType(); echo $value instanceof $class ? 'yes' : 'no';"
        ),
        "yes"
    );
}

#[test]
fn instanceof_accepts_a_runtime_class_name_from_an_object_property() {
    assert_eq!(
        run_php(
            "<?php class PropertyRuntimeType {} class TypeHolder { public string $type = PropertyRuntimeType::class; } $holder = new TypeHolder(); $value = new PropertyRuntimeType(); echo $value instanceof $holder->type ? 'yes:' : 'no:'; echo !$value instanceof $holder->type ? 'bad' : 'negated';"
        ),
        "yes:negated"
    );
}

#[test]
fn interface_implementation_may_add_a_typed_optional_parameter() {
    assert_eq!(
        run_php(
            "<?php interface Clearable { public function clear(): bool; } class Cache implements Clearable { public function clear(string $prefix = ''): bool { return $prefix === ''; } } echo (new Cache())->clear() ? 'yes' : 'no';"
        ),
        "yes"
    );
}

#[test]
fn reflection_property_reads_initialization_and_writes_private_storage() {
    assert_eq!(
        run_php(
            "<?php class ReflectedProperty { private string $value; } $object = new ReflectedProperty(); $property = new ReflectionProperty($object, 'value'); echo $property->isInitialized($object) ? 'set' : 'unset'; $property->setValue($object, 'ok'); echo ':' . $property->getValue($object);"
        ),
        "unset:ok"
    );
}

#[test]
fn reflection_class_exposes_name_parent_empty_attributes_and_interfaces() {
    assert_eq!(
        run_php(
            "<?php interface ReflectedInterface {} class ReflectedParent implements ReflectedInterface {} class ReflectedChild extends ReflectedParent {} $class = new ReflectionClass(ReflectedChild::class); $interfaces = class_implements($class->name); echo $class->name . ':' . count($class->getAttributes(null, ReflectionAttribute::IS_INSTANCEOF)) . ':' . $class->getParentClass()->name . ':' . (isset($interfaces['ReflectedInterface']) ? 'yes' : 'no');"
        ),
        "ReflectedChild:0:ReflectedParent:yes"
    );
}

#[test]
fn class_parents_reports_nearest_first_for_names_and_objects() {
    assert_eq!(
        run_php(
            "<?php class ParentRoot {} class ParentMiddle extends ParentRoot {} class ParentLeaf extends ParentMiddle {} echo implode(',', class_parents(ParentLeaf::class)), '|', implode(',', class_parents(new ParentLeaf(), false));"
        ),
        "ParentMiddle,ParentRoot|ParentMiddle,ParentRoot"
    );
}

#[test]
fn reserved_keyword_method_name_is_callable() {
    assert_eq!(
        run_php(
            "<?php class KeywordRenderer { public function include(string $path): string { return 'included:' . $path; } } echo (new KeywordRenderer())->include('view.php');"
        ),
        "included:view.php"
    );
}

#[test]
fn empty_anonymous_class_preserves_parent_and_interfaces() {
    assert_eq!(
        run_php(
            "<?php interface AnonymousMarker {} class AnonymousParent { public function value(): string { return 'ok'; } } $object = new class extends AnonymousParent implements AnonymousMarker {}; echo $object->value() . ':' . ($object instanceof AnonymousMarker ? 'yes' : 'no');"
        ),
        "ok:yes"
    );
}

#[test]
fn anonymous_class_compiles_promoted_properties_and_methods() {
    assert_eq!(
        run_php(
            "<?php class AnonymousHeader { public array $values = ['ok' => true]; } $checker = new class(new AnonymousHeader()) { public function __construct(private AnonymousHeader $header) {} public function __invoke(string $key): bool { return array_key_exists($key, $this->header->values); } }; echo $checker('ok') ? 'yes' : 'no';"
        ),
        "yes"
    );
}

#[test]
fn dynamic_new_accepts_property_and_dimension_class_expression() {
    assert_eq!(
        run_php(
            "<?php class DynamicallyMade { public string $value = 'ok'; } class DynamicFactory { public array $types = ['result' => DynamicallyMade::class]; public function make(): object { return new $this->types['result'](); } } echo (new DynamicFactory())->make()->value;"
        ),
        "ok"
    );
}

#[test]
fn test_class_basic_property() {
    assert_eq!(
        run_php(
            r#"<?php
class Dog {
    public $name;
}
$d = new Dog();
$d->name = "Rex";
echo $d->name;
"#
        ),
        "Rex"
    );
}

#[test]
fn test_class_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Dog {
    public $name;
    public function bark() {
        echo "Woof from " . $this->name;
    }
}
$d = new Dog();
$d->name = "Rex";
$d->bark();
"#
        ),
        "Woof from Rex"
    );
}

#[test]
fn test_class_method_with_params() {
    assert_eq!(
        run_php(
            r#"<?php
class Calculator {
    public function add($a, $b) {
        return $a + $b;
    }
}
$c = new Calculator();
echo $c->add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_class_scalar_long_method_nested_calls() {
    assert_eq!(
        run_php(
            r#"<?php
class Calculator {
    public function add($a, $b) { return $a + $b; }
    public function mul($a, $b) { return $a * $b; }
}
$c = new Calculator();
echo $c->add(2, $c->mul(3, 4));
"#
        ),
        "14"
    );
}

#[test]
fn test_class_composed_scalar_call_guards_polymorphic_dispatch() {
    assert_eq!(
        run_php(
            r#"<?php
class AddMath {
    public function combine($a, $b) { return $a + $b; }
    public function inner($a, $b) { return $a * $b; }
}
class SubMath {
    public function combine($a, $b) { return $a - $b; }
    public function inner($a, $b) { return $a + $b; }
}
function calculate($math, $value) {
    return $math->combine($value, $math->inner($value, 2));
}
$add = new AddMath();
$sub = new SubMath();
echo calculate($add, 3) . ':' . calculate($add, 4) . '|';
echo calculate($sub, 3) . ':' . calculate($sub, 4) . '|';
echo calculate($add, 5);
"#
        ),
        "9:12|-2:-2|15"
    );
}

#[test]
fn test_object_long_method_side_exits_across_property_layouts_and_types() {
    assert_eq!(
        run_php(
            r#"<?php
class RequestA {
    public $level;
    public $subtotal;
}
class RequestB {
    public $subtotal;
    public $level;
}
class RequestC {
    public $level;
    public $subtotal;
}
class Policy {
    public function rate($request) {
        $rate = 150;
        if ($request->level >= 3) $rate = $rate + 250;
        if ($request->subtotal >= 20000) $rate = $rate + 175;
        return $rate;
    }
}
function invoke($policy, $request) {
    return $policy->rate($request);
}
$policy = new Policy();
$a = new RequestA(); $a->level = 4; $a->subtotal = 30000;
$b = new RequestB(); $b->level = 1; $b->subtotal = 30000;
$c = new RequestC(); $c->level = 4.0; $c->subtotal = 100.0;
echo invoke($policy, $a) . ':' . invoke($policy, $a) . '|';
echo invoke($policy, $b) . ':' . invoke($policy, $b) . '|';
echo invoke($policy, $c) . ':' . invoke($policy, $c) . '|';
echo invoke($policy, $a);
"#
        ),
        "575:575|325:325|400:400|575"
    );
}

#[test]
fn test_object_long_method_handles_string_branches_and_intdiv() {
    assert_eq!(
        run_php(
            r#"<?php
class TaxPolicy {
    public function amount($net, $region) {
        if ($region == 'EU') return intdiv($net * 2100, 10000);
        if ($region == 'US') return intdiv($net * 725, 10000);
        return intdiv($net * 1200, 10000);
    }
}
function tax($policy, $net, $region) {
    return $policy->amount($net, $region);
}
function taxByReference($policy, $net, &$region) {
    return $policy->amount($net, $region);
}
$policy = new TaxPolicy();
$eu = 'EU';
echo tax($policy, 10000, 'EU') . ':';
echo tax($policy, 10000, 'US') . ':';
echo tax($policy, 10000, 'ROW') . ':';
echo taxByReference($policy, 10000, $eu);
"#
        ),
        "2100:725:1200:2100"
    );
}

#[test]
fn test_object_long_property_argument_rechecks_layout_and_dynamic_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class TaxPolicy {
    public function amount($net, $region) {
        if ($region == 'EU') return intdiv($net * 2100, 10000);
        if ($region == 'US') return intdiv($net * 725, 10000);
        return intdiv($net * 1200, 10000);
    }
}
class RequestA { public $region; }
class RequestB { public $padding; public $region; }
#[AllowDynamicProperties]
class DynamicRequest {}
function quoteTax($policy, $request) {
    return $policy->amount(10000, $request->region);
}
$policy = new TaxPolicy();
$a = new RequestA(); $a->region = 'EU';
$b = new RequestB(); $b->region = 'US';
$dynamic = new DynamicRequest(); $dynamic->region = 'ROW';
echo quoteTax($policy, $a) . ':' . quoteTax($policy, $a) . '|';
echo quoteTax($policy, $b) . ':' . quoteTax($policy, $b) . '|';
echo quoteTax($policy, $dynamic) . ':' . quoteTax($policy, $dynamic) . '|';
echo quoteTax($policy, $a);
"#
        ),
        "2100:2100|725:725|1200:1200|2100"
    );
}

#[test]
fn test_class_multiple_properties() {
    assert_eq!(
        run_php(
            r#"<?php
class Person {
    public $first;
    public $last;
}
$p = new Person();
$p->first = "John";
$p->last = "Doe";
echo $p->first . " " . $p->last;
"#
        ),
        "John Doe"
    );
}

#[test]
fn test_class_method_using_this() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public $count;
    public function increment() {
        $this->count = $this->count + 1;
    }
    public function get() {
        return $this->count;
    }
}
$c = new Counter();
$c->count = 0;
$c->increment();
$c->increment();
$c->increment();
echo $c->get();
"#
        ),
        "3"
    );
}

#[test]
fn test_class_multiple_methods() {
    assert_eq!(
        run_php(
            r#"<?php
class Greeter {
    public $name;
    public function hello() {
        echo "Hello " . $this->name;
    }
    public function bye() {
        echo "Bye " . $this->name;
    }
}
$g = new Greeter();
$g->name = "World";
$g->hello();
echo " ";
$g->bye();
"#
        ),
        "Hello World Bye World"
    );
}

#[test]
fn test_class_multiple_instances() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public $value;
}
$a = new Box();
$a->value = 10;
$b = new Box();
$b->value = 20;
echo $a->value . " " . $b->value;
"#
        ),
        "10 20"
    );
}

#[test]
fn test_new_object_creates_instance() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
$f = new Foo();
echo "ok";
"#
        ),
        "ok"
    );
}

#[test]
fn test_class_method_return() {
    assert_eq!(
        run_php(
            r#"<?php
class Math {
    public function square($x) {
        return $x * $x;
    }
}
$m = new Math();
echo $m->square(7);
"#
        ),
        "49"
    );
}

#[test]
fn test_class_this_property_write_in_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Setter {
    public $val;
    public function set($v) {
        $this->val = $v;
    }
}
$s = new Setter();
$s->set("hello");
echo $s->val;
"#
        ),
        "hello"
    );
}

#[test]
fn test_class_property_default_int() {
    assert_eq!(
        run_php(
            r#"<?php
class Config {
    public $timeout = 30;
}
$c = new Config();
echo $c->timeout;
"#
        ),
        "30"
    );
}

#[test]
fn test_class_property_default_string() {
    assert_eq!(
        run_php(
            r#"<?php
class Config {
    public $name = "default";
}
$c = new Config();
echo $c->name;
"#
        ),
        "default"
    );
}

#[test]
fn test_class_property_default_override() {
    assert_eq!(
        run_php(
            r#"<?php
class Config {
    public $x = 10;
}
$c = new Config();
$c->x = 42;
echo $c->x;
"#
        ),
        "42"
    );
}

#[test]
fn test_class_property_default_bool() {
    assert_eq!(
        run_php(
            r#"<?php
class Flags {
    public $active = true;
    public $deleted = false;
}
$f = new Flags();
echo $f->active;
"#
        ),
        "1"
    );
}

#[test]
fn test_class_property_no_default_is_null() {
    assert_eq!(
        run_php(
            r#"<?php
class Empty2 {
    public $x;
}
$e = new Empty2();
echo $e->x ?? "null";
"#
        ),
        "null"
    );
}

#[test]
fn test_borrowed_object_parameter_materializes_before_nested_by_ref_rebind() {
    assert_eq!(
        run_php(
            r#"<?php
class BorrowBox {
    public $value;
    public function __construct($value) { $this->value = $value; }
}
function replaceBorrowBox(&$box) {
    $box = new BorrowBox(9);
}
function observeAndReplaceBorrowBox($box) {
    $before = $box->value;
    replaceBorrowBox($box);
    return $before . ':' . $box->value;
}
$original = new BorrowBox(3);
for ($i = 0; $i < 20; $i++) {
    $last = observeAndReplaceBorrowBox($original);
}
echo $last . '|' . $original->value;
"#
        ),
        "3:9|3"
    );
}

#[test]
fn new_static_in_a_static_closure_keeps_the_forwarded_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
class LateFactory {
    public static function make(): static {
        $factory = static function(): static { return new static(); };
        return $factory();
    }
    public function kind() { return 'base'; }
}
class LateFactoryChild extends LateFactory {
    public function kind() { return 'child'; }
}
echo LateFactoryChild::make()->kind();
"#,
        ),
        "child"
    );
}

#[test]
fn variable_class_instantiation_rekeys_the_constructor_cache() {
    assert_eq!(
        run_php(
            r#"<?php
class DynamicFirst {
    public function __construct(public $value) {}
}
class DynamicSecond {
    public function __construct(public $value) {}
}
function make($class, $value) { return new $class($value); }
echo make(DynamicFirst::class, 'first')->value, '|';
echo make(DynamicSecond::class, 'second')->value;
"#,
        ),
        "first|second"
    );
}

#[test]
fn repeated_anonymous_new_reuses_the_registered_class_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$firstClass = null;
$observed = [];
for ($index = 0; $index < 3; $index++) {
    $object = new class($index) {
        public function __construct(public $value) {}
    };
    $class = get_class($object);
    if ($firstClass === null) { $firstClass = $class; }
    $observed[] = ($class === $firstClass ? 'same' : 'different') . ':' . $object->value;
}
echo implode('|', $observed);
"#,
        ),
        "same:0|same:1|same:2"
    );
}

#[test]
fn nested_method_arguments_do_not_inherit_the_callers_pending_call_chain() {
    assert_eq!(
        run_php(
            r#"<?php
class NestedValues {
    public function __construct(private array $values) {}
    public function getValues(): array { return $this->values; }
    public function setValues(array $values): void { $this->values = $values; }
}
class NestedProcessor {
    public function processValue(mixed $value, int $root = 0, int $level = 0): mixed {
        if ($value instanceof NestedValues) {
            $value->setValues($this->processValue($value->getValues(), 1, 1));
        } elseif (is_array($value)) {
            foreach ($value as $key => $entry) {
                $value[$key] = $this->processValue($entry, $root, $level + 1);
            }
        }
        return $value;
    }
}
$value = new NestedValues([[1], [2, 3]]);
$processor = new NestedProcessor();
for ($index = 0; $index < 32; ++$index) {
    $processor->processValue($value);
}
echo count($value->getValues()), ':', $value->getValues()[1][1];
"#,
        ),
        "2:3"
    );
}

#[test]
fn dynamic_object_property_reference_preserves_nested_array_writes() {
    let output = run_php(
        r#"<?php
class DynamicPropertyReferenceFixture {
    private array $beforeOptimizationPasses = [0 => ['initial']];

    public function add(string $property): void {
        $passes = &$this->$property;
        $passes[0][] = 'added';
        $passes[10] = ['priority'];
    }

    public function values(): array {
        return $this->beforeOptimizationPasses;
    }
}

$fixture = new DynamicPropertyReferenceFixture();
$fixture->add('beforeOptimizationPasses');
var_dump($fixture->values());
"#,
    );

    assert_eq!(
        output,
        concat!(
            "array(2) {\n",
            "  [0]=>\n",
            "  array(2) {\n",
            "    [0]=>\n",
            "    string(7) \"initial\"\n",
            "    [1]=>\n",
            "    string(5) \"added\"\n",
            "  }\n",
            "  [10]=>\n",
            "  array(1) {\n",
            "    [0]=>\n",
            "    string(8) \"priority\"\n",
            "  }\n",
            "}\n",
        )
    );
}

#[test]
fn array_element_reference_preserves_writes() {
    assert_eq!(
        run_php(
            r#"<?php
$arguments = [['initial']];
$argumentAtIndex = &$arguments[0];
$argumentAtIndex[] = 'added';
$argumentAtIndex[4] = 'priority';
echo count($arguments[0]), ':', $arguments[0][0], ':', $arguments[0][1], ':', $arguments[0][4];
"#,
        ),
        "3:initial:added:priority"
    );
}

#[test]
fn get_object_vars_preserves_scope_dynamic_keys_cow_and_references() {
    assert_eq!(
        run_php(
            r#"<?php
class ObjectVarsParent {
    private $shadow = 'parent';
    protected $guarded = 'protected';
    public array $normal = ['initial'];
    public $linked;
    public int $typed;
    public function inspect(object $object): array { return get_object_vars($object); }
}
#[AllowDynamicProperties]
class ObjectVarsChild extends ObjectVarsParent { public $shadow = 'child'; }
$reference = ['initial'];
$object = new ObjectVarsChild;
$object->linked = &$reference;
$object->{123} = 'numeric';
$outside = get_object_vars($object);
$inside = $object->inspect($object);
$object->normal[] = 'later';
$object->linked[] = 'alias';
echo $outside['shadow'], '|', isset($outside['guarded']) ? 'bad' : 'hidden', '|';
echo $inside['shadow'], '|', $inside['guarded'], '|';
echo implode(',', $outside['normal']), '|', implode(',', $outside['linked']), '|';
echo implode(',', $reference), '|', $outside[123], '|';
var_dump(array_key_exists('typed', $outside), get_object_vars(function () {}));
try { get_object_vars(42); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "child|hidden|parent|protected|initial|initial,alias|initial,alias|numeric|",
            "bool(false)\narray(0) {\n}\n",
            "get_object_vars(): Argument #1 ($object) must be of type object, int given\n",
        )
    );
}
