mod common;

use common::{run_php, run_php_expect_error, run_php_with_source_context};

#[test]
fn property_magic_constant_covers_defaults_hooks_and_attribute_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
#[Attribute]
class PropertyName {
    public function __construct(public string $value) {}
}

class NamedProperty {
    #[PropertyName(__PROPERTY__)]
    public string $item = __PROPERTY__ {
        #[PropertyName(__PROPERTY__)]
        get {
            echo __PROPERTY__, '|';
            return $this->item;
        }
        set (#[PropertyName(__PROPERTY__)] string $value) {
            $this->item = $value;
        }
    }
}

$object = new NamedProperty();
echo $object->item, '|';
$property = new ReflectionProperty(NamedProperty::class, 'item');
echo $property->getAttributes()[0]->getArguments()[0], '|';
echo $property->getHook(PropertyHookType::Get)->getAttributes()[0]->getArguments()[0], '|';
echo $property->getHook(PropertyHookType::Set)->getParameters()[0]->getAttributes()[0]->getArguments()[0];
"#,
        ),
        "item|item|item|item|item"
    );
}

#[test]
fn property_magic_constant_is_empty_outside_the_immediate_property_scope() {
    assert_eq!(
        run_php(
            r#"<?php
echo '[', __PROPERTY__, ']';
function ordinaryFunction() { echo '[', __PROPERTY__, ']'; }
ordinaryFunction();

class PropertyBoundary {
    public function ordinaryMethod() { echo '[', __PROPERTY__, ']'; }
    public string $item {
        get {
            $nested = fn () => __PROPERTY__;
            echo '[', $nested(), ']';
            return __PROPERTY__;
        }
    }
}

$object = new PropertyBoundary();
$object->ordinaryMethod();
echo '[', $object->item, ']';
"#,
        ),
        "[][][][[]item]"
    );
}

#[test]
fn getter_hook_runs_and_getter_only_property_rejects_writes() {
    assert_eq!(
        run_php(
            r#"<?php
class Reading {
    public $answer {
        get { return 6 * 7; }
    }
}
$reading = new Reading();
var_dump($reading->answer);
try { $reading->answer = 9; } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "int(42)\nProperty Reading::$answer is read-only\n"
    );
}

#[test]
fn getter_hook_reentrance_reads_backing_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public $value = 40 {
        get {
            echo __FUNCTION__, " ", __METHOD__, "\n";
            return $this->value + 2;
        }
    }
}
$counter = new Counter();
$counter->value = 40;
var_dump($counter->value);
"#,
        ),
        "$value::get Counter::$value::get\nint(42)\n"
    );
}

#[test]
fn getter_hook_inheritance_uses_method_variance_contract() {
    let error = run_php_expect_error(
        r#"<?php
class ParentReading { public int $value { get { return 1; } } }
class ChildReading extends ParentReading { public int|float $value { get { return 1; } } }
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of ChildReading::$value::get(): int|float must be compatible with ParentReading::$value::get(): int\")"
    );
}

#[test]
fn setter_hook_parameter_type_is_contravariant_with_the_property_type() {
    assert_eq!(
        run_php(
            r#"<?php
interface SetterBase {}
interface SetterChild extends SetterBase {}
class ContravariantSetter {
    public SetterChild $value { set(SetterBase $incoming) {} }
}
class LateObjectSetter {
    public LaterObjectContract $value { set(object $incoming) {} }
}
interface LaterObjectContract {}
interface AlreadyKnownSetterBase {}
class OneKnownLateSetter {
    public OneKnownLateChild $value { set(AlreadyKnownSetterBase $incoming) {} }
}
interface OneKnownLateChild extends AlreadyKnownSetterBase {}
echo "compatible";
"#,
        ),
        "compatible"
    );

    for (source, expected) in [
        (
            r#"<?php class UntypedProperty { public $value { set(string $incoming) {} } }"#,
            "Type of parameter $incoming of hook UntypedProperty::$value::set must be compatible with property type",
        ),
        (
            r#"<?php class UntypedSetter { public string $value { set($incoming) {} } }"#,
            "Type of parameter $incoming of hook UntypedSetter::$value::set must be compatible with property type",
        ),
        (
            r#"<?php class NarrowSetter { public string|array $value { set(string $incoming) {} } }"#,
            "Type of parameter $incoming of hook NarrowSetter::$value::set must be compatible with property type",
        ),
        (
            r#"<?php
class LateTypes { public SetterContract $value { set(OtherContract $incoming) {} } }
interface SetterContract {}
interface OtherContract {}
"#,
            "Type of parameter $incoming of hook LateTypes::$value::set must be compatible with property type",
        ),
    ] {
        assert!(format!("{:?}", run_php_expect_error(source)).contains(expected));
    }
}

#[test]
fn property_hook_inheritance_uses_directional_type_variance() {
    let output = run_php(
        r#"<?php
class GetterParent { public int|float $value { get => 42.0; } }
class GetterChild extends GetterParent { public int $value { get => 42; set {} } }
class SetterParent { public int $value { set {} } }
class SetterChild extends SetterParent { public int|string $value { get => 42; set {} } }
class NamedGetterParent {
    public NamedGetterBase $value { get { throw new Exception; } }
}
class NamedGetterChild extends NamedGetterParent {
    public NamedGetterLeaf $value { get { throw new Exception; } set {} }
}
interface NamedGetterBase {}
interface NamedGetterLeaf extends NamedGetterBase {}
class NamedSetterParent { public NamedSetterLeaf $value { set {} } }
class NamedSetterChild extends NamedSetterParent {
    public NamedSetterBase $value { get { throw new Exception; } set {} }
}
interface NamedSetterBase {}
interface NamedSetterLeaf extends NamedSetterBase {}
echo "ok\n";
"#,
    );
    assert_eq!(output, "ok\n");
}

#[test]
fn backed_property_hook_inheritance_keeps_its_type_invariant() {
    let error = run_php_expect_error(
        r#"<?php
class BackedParent { public int|float $value { get => $this->value; } }
class BackedChild extends BackedParent { public int $value { get => 42; } }
"#,
    );
    assert!(
        format!("{error:?}")
            .contains("Type of BackedChild::$value must be int|float (as in class BackedParent)")
    );
}

#[test]
fn property_prototypes_define_set_capability_and_protected_family_scope() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class ReadPrototype { abstract protected int $value { get; } }
class ReadLeft extends ReadPrototype { protected int $value = 11; }
class ReadRight extends ReadPrototype {
    protected int $value = 22;
    public static function read(ReadPrototype $object): int { return $object->value; }
}

abstract class WritePrototype {
    abstract public protected(set) int $value { get; set; }
}
class WriteLeft extends WritePrototype {
    public protected(set) int $value { get => 40; set {} }
}
class WriteRight extends WritePrototype {
    public int $value = 1;
    public static function increment(WritePrototype $object): int {
        return $object->value += 2;
    }
}

class UnrelatedLeft { protected int $value = 33; }
class UnrelatedRight {
    protected int $value = 44;
    public static function read(UnrelatedLeft $object): string {
        try { return (string) $object->value; } catch (Error) { return 'blocked'; }
    }
}

abstract class GetterOnly { abstract public int $value { get; } }
class AddsProtectedSetter extends GetterOnly {
    public protected(set) int $value { get => 1; set {} }
}

echo ReadRight::read(new ReadLeft), "\n";
echo WriteRight::increment(new WriteLeft), "\n";
echo UnrelatedRight::read(new UnrelatedLeft), "\n";
echo new AddsProtectedSetter()->value, "\n";
"#,
        ),
        "11\n42\nblocked\n1\n"
    );
}

#[test]
fn plain_child_storage_inherits_concrete_prototype_hooks() {
    assert_eq!(
        run_php(
            r#"<?php
class GetterPrototype { public $value { get => 42; } }
class StoredGetter extends GetterPrototype { public $value = 1; }
$getter = new StoredGetter;
$getter->value = 7;
var_dump($getter->value);

class HookPrototype {
    public $value {
        get { echo "get\n"; return 40; }
        set { echo "set:$value\n"; }
    }
}
class StoredHooks extends HookPrototype { public $value; }
$hooks = new StoredHooks;
$hooks->value = 9;
var_dump($hooks->value);
"#,
        ),
        "int(42)\nset:9\nget\nint(40)\n"
    );
}

#[test]
fn reference_getter_marks_only_its_own_property_access_as_backing_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class BackedItems {
    public array $items = [] {
        &get { echo "backed-get\n"; return $this->items; }
    }
}
$backed = new BackedItems;
$backed->items[] = 'x';
var_dump($backed->items);

class BackedScalar {
    public $value = 0 {
        &get { echo "scalar-get\n"; return $this->value; }
    }
}
$scalar = new BackedScalar;
try {
    $scalar->value = &$replacement;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}

class VirtualItems {
    private array $storage = [];
    public array $items {
        &get { echo "virtual-get\n"; return $this->storage; }
    }
}
$virtual = new VirtualItems;
$virtual->items[] = 'y';
var_dump($virtual->items);
$bound =& $virtual->items[];
$bound = 'bound';
var_dump($virtual->items);

class VirtualItemsWithSet {
    private array $storage = [];
    public private(set) array $items {
        &get { echo "virtual-set-get\n"; return $this->storage; }
        set { echo "virtual-set-set\n"; $this->storage = $value; }
    }
}
$virtualWithSet = new VirtualItemsWithSet;
$virtualWithSet->items[] = 'z';
var_dump($virtualWithSet->items);

class OrdinaryPrivateSetReference {
    public private(set) array $items = [];
    public function bind(array &$items): void { $this->items =& $items; }
}
$ordinary = new OrdinaryPrivateSetReference;
$external = [];
$ordinary->bind($external);
try {
    $ordinary->items[] = 'blocked';
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
var_dump($external, $ordinary->items);
"#,
        ),
        concat!(
            "backed-get\n",
            "backed-get\n",
            "array(1) {\n  [0]=>\n  string(1) \"x\"\n}\n",
            "scalar-get\n",
            "Cannot assign by reference to overloaded object\n",
            "virtual-get\n",
            "virtual-get\n",
            "array(1) {\n  [0]=>\n  string(1) \"y\"\n}\n",
            "virtual-get\n",
            "virtual-get\n",
            "array(2) {\n",
            "  [0]=>\n  string(1) \"y\"\n",
            "  [1]=>\n  &string(5) \"bound\"\n}\n",
            "virtual-set-get\n",
            "virtual-set-get\n",
            "array(1) {\n  [0]=>\n  string(1) \"z\"\n}\n",
            "Cannot indirectly modify private(set) property ",
            "OrdinaryPrivateSetReference::$items from global scope\n",
            "array(0) {\n}\n",
            "array(0) {\n}\n",
        )
    );
}

#[test]
fn override_properties_and_hooks_match_only_effective_parent_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
interface RequiredProperties {
    public mixed $plain { get; }
    public mixed $promoted { get; }
}
class HookParent {
    public mixed $hooked = 'parent' {
        get => $this->hooked;
        set => $value;
    }
    public mixed $implicitlyReadable {
        set { $this->implicitlyReadable = $value; }
    }
}
class HookChild extends HookParent implements RequiredProperties {
    #[Override]
    public mixed $plain = 'plain';

    public function __construct(
        #[Override]
        public mixed $promoted = 'promoted',
    ) {}

    public mixed $hooked {
        #[Override]
        get => parent::$hooked::get();
        #[Override]
        set => parent::$hooked::set($value);
    }

    public mixed $implicitlyReadable {
        #[Override]
        get => parent::$implicitlyReadable::get();
    }
}
$child = new HookChild;
echo $child->plain, ':', $child->promoted, ':';
$parameter = (new ReflectionMethod(HookChild::class, '__construct'))->getParameters()[0];
$property = new ReflectionProperty(HookChild::class, 'promoted');
echo count($parameter->getAttributes(Override::class)), ':';
$attribute = $property->getAttributes(Override::class)[0];
echo $attribute->getTarget(), ':', get_class($attribute->newInstance()), ':';
$hooked = new ReflectionProperty(HookChild::class, 'hooked');
foreach ($hooked->getHooks() as $hook) {
    $attribute = $hook->getAttributes(Override::class)[0];
    echo $attribute->getTarget(), ':', get_class($attribute->newInstance()), ':';
}
"#,
        ),
        "plain:promoted:0:8:Override:4:Override:4:Override:"
    );

    let cases = [
        (
            "<?php class MissingProperty { #[Override] public mixed $value; }",
            "MissingProperty::$value",
            "property",
        ),
        (
            "<?php class PrivatePropertyParent { private mixed $value; } class PrivatePropertyChild extends PrivatePropertyParent { #[Override] public mixed $value; }",
            "PrivatePropertyChild::$value",
            "property",
        ),
        (
            "<?php trait ConcreteProperty { public mixed $value; } class ConcretePropertyChild { use ConcreteProperty; #[Override] public mixed $value; }",
            "ConcretePropertyChild::$value",
            "property",
        ),
        (
            "<?php trait MarkedProperty { #[Override] public mixed $value; } class MarkedPropertyChild { use MarkedProperty; }",
            "MarkedPropertyChild::$value",
            "property",
        ),
        (
            "<?php class SetterOnly { public mixed $value { set {} } } class MissingGetter extends SetterOnly { public mixed $value { #[Override] get => 1; } }",
            "MissingGetter::$value::get()",
            "method",
        ),
        (
            "<?php class DelayedHook { public mixed $value { #[DelayedTargetValidation] #[Override] get => 1; } }",
            "DelayedHook::$value::get()",
            "method",
        ),
    ];
    for (source, member, kind) in cases {
        let error = run_php_expect_error(source);
        assert_eq!(
            format!("{error:?}"),
            format!(
                "Fatal(\"{member} has #[\\\\Override] attribute, but no matching parent {kind} exists\")"
            )
        );
    }
}

#[test]
fn setter_hook_inheritance_diagnostics_include_the_implicit_void_contract() {
    let error = run_php_expect_error(
        r#"<?php
interface SetterParentType {}
interface SetterChildType extends SetterParentType {}
class ParentSetter {
    public SetterChildType $value { set(SetterParentType $incoming) {} }
}
class ChildSetter extends ParentSetter {
    public SetterChildType $value { set(SetterChildType $incoming) {} }
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of ChildSetter::$value::set(SetterChildType $incoming): void must be compatible with ParentSetter::$value::set(SetterParentType $incoming): void\")"
    );
}

#[test]
fn synthetic_plain_property_setter_keeps_its_parent_call_value_and_void_contract() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class AbstractProperty {
    abstract public $value { get; set; }
}
class PlainProperty extends AbstractProperty { public $value; }

class ParentStorage { public $value; }
class ChildHook extends ParentStorage {
    public $value { set { var_dump(parent::$value::set($value)); } }
}
(new ChildHook())->value = 42;
echo get_class(new PlainProperty());
"#,
        ),
        "int(42)\nPlainProperty"
    );
}

#[test]
fn getter_hook_is_not_exposed_as_an_ordinary_method() {
    assert_eq!(
        run_php(
            r#"<?php
class SecretHook {
    public $value { get { return 42; } }
    public function visible() {}
}
var_dump(get_class_methods(SecretHook::class));
"#,
        ),
        "array(1) {\n  [0]=>\n  string(7) \"visible\"\n}\n"
    );
}

#[test]
fn setter_hook_receives_assigned_value_and_virtual_property_is_write_only() {
    assert_eq!(
        run_php(
            r#"<?php
class Sink {
    public $last;
    public $value { set { $this->last = $value * 2; } }
}
$sink = new Sink();
$sink->value = 21;
var_dump($sink->last);
try { var_dump($sink->value); } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { var_dump(isset($sink->value)); } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "int(42)\nProperty Sink::$value is write-only\nProperty Sink::$value is write-only\n"
    );
}

#[test]
fn explicit_setter_parameter_can_transform_backing_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class Labels {
    public string $value {
        set(string|array $incoming) {
            $this->value = is_array($incoming) ? join(':', $incoming) : $incoming;
        }
    }
}
$labels = new Labels();
var_dump($labels->value = ['a', 'b']);
var_dump($labels->value);
"#,
        ),
        "array(2) {\n  [0]=>\n  string(1) \"a\"\n  [1]=>\n  string(1) \"b\"\n}\nstring(3) \"a:b\"\n"
    );
}

#[test]
fn accesses_inside_either_hook_use_the_same_backing_slot() {
    assert_eq!(
        run_php(
            r#"<?php
class Guarded {
    public $value {
        get { $this->value = 40; return $this->value + 2; }
        set { $this->value = $value; }
    }
}
$guarded = new Guarded();
var_dump($guarded->value);
$guarded->value = 9;
var_dump($guarded->value);
"#,
        ),
        "int(42)\nint(42)\n"
    );
}

#[test]
fn arrow_getter_returns_its_expression() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrowReading {
    public int $base = 20;
    public int $answer { get => $this->base + 22; }
}
$reading = new ArrowReading();
var_dump($reading->answer);
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn arrow_setter_stores_transformed_value_but_assignment_returns_input() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrowLabel {
    public string $value {
        get => $this->value;
        set => strtoupper($value);
    }
}
$label = new ArrowLabel();
var_dump($label->value = 'mixed');
var_dump($label->value);
"#,
        ),
        "string(5) \"mixed\"\nstring(5) \"MIXED\"\n"
    );
}

#[test]
fn final_property_rejects_child_redeclaration() {
    let error = run_php_expect_error(
        r#"<?php
class FixedStorage { public final int $value = 1; }
class ReplacementStorage extends FixedStorage { public int $value = 2; }
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Cannot override final property FixedStorage::$value\")"
    );
}

#[test]
fn final_getter_rejects_child_hook_override() {
    let error = run_php_expect_error(
        r#"<?php
class FixedReading { public int $value { final get => 1; } }
class ReplacementReading extends FixedReading { public int $value { get => 2; } }
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Cannot override final property hook FixedReading::$value::get()\")"
    );
}

#[test]
fn final_private_property_and_hook_are_rejected() {
    for (source, expected) in [
        (
            "<?php class HiddenStorage { final private int $value; }",
            "Property cannot be both final and private",
        ),
        (
            "<?php class HiddenReading { private int $value { final get; } }",
            "Property hook cannot be both final and private",
        ),
        (
            "<?php class ImpossibleReading { public int $value { final get; } }",
            "Property hook cannot be both abstract and final",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(
            format!("{error:?}").contains(expected),
            "unexpected error: {error:?}"
        );
    }
}

#[test]
fn abstract_getter_can_be_implemented_while_inheriting_a_concrete_setter() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class AbstractReading {
    public abstract int $value {
        get;
        set { $this->value = $value; }
    }
}
class ConcreteReading extends AbstractReading {
    public int $value { get => $this->value; }
}
$reading = new ConcreteReading();
$reading->value = 42;
var_dump($reading->value);
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn plain_property_implements_both_abstract_hook_requirements() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class AbstractStorage {
    public abstract int $value { get; set; }
}
class ConcreteStorage extends AbstractStorage {
    public int $value = 42;
}
var_dump((new ConcreteStorage())->value);
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn concrete_class_reports_all_unimplemented_property_hooks() {
    let error = run_php_expect_error(
        r#"<?php
class IncompleteStorage {
    public abstract int $value { get; set; }
}
"#,
    );
    let rendered = format!("{error:?}");
    assert!(rendered.contains("contains 2 abstract methods"));
    assert!(rendered.contains("IncompleteStorage::$value::get"));
    assert!(rendered.contains("IncompleteStorage::$value::set"));
}

#[test]
fn interface_property_hooks_accept_plain_and_readonly_get_implementations() {
    assert_eq!(
        run_php(
            r#"<?php
interface ReadableValue { public int $value { get; } }
class FixedValue implements ReadableValue {
    public function __construct(public readonly int $value) {}
}
var_dump((new FixedValue(42))->value);
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn readonly_property_does_not_implement_interface_set_hook() {
    let error = run_php_expect_error(
        r#"<?php
interface MutableValue { public int $value { get; set; } }
class FixedValue implements MutableValue {
    public function __construct(public readonly int $value) {}
}
"#,
    );
    assert!(format!("{error:?}").contains(
        "Set access level of FixedValue::$value must be omitted (as in class MutableValue)"
    ));
}

#[test]
fn reference_getter_exposes_the_returned_backing_alias() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferencedValue {
    private int $storage = 41;
    public int $value {
        &get => $this->storage;
        set => $this->storage = $value;
    }
}
function increment(int &$value): void { $value++; }
$object = new ReferencedValue();
$alias = &$object->value;
$alias++;
increment($object->value);
var_dump($object->value);
"#,
        ),
        "int(43)\n"
    );
}

#[test]
fn plain_property_implements_reference_getter_interface() {
    assert_eq!(
        run_php(
            r#"<?php
interface ReferencedProperty { public int $value { &get; } }
class PlainValue implements ReferencedProperty { public int $value = 41; }
$object = new PlainValue();
$alias = &$object->value;
$alias++;
var_dump($object->value);
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn value_getter_cannot_implement_reference_getter_interface() {
    let error = run_php_expect_error(
        r#"<?php
interface ReferencedProperty { public int $value { &get; } }
class ValueGetter implements ReferencedProperty {
    public int $value { get => $this->value; }
}
"#,
    );
    let rendered = format!("{error:?}");
    assert!(rendered.contains("Declaration of ValueGetter::$value::get(): int"));
    assert!(rendered.contains("compatible with & ReferencedProperty::$value::get(): int"));
}

#[test]
fn set_hook_parameter_cannot_be_passed_by_reference() {
    let error = run_php_expect_error(
        r#"<?php
class InvalidSetter { public $value { set(&$incoming) {} } }
"#,
    );
    assert!(format!("{error:?}").contains(
        "Parameter $incoming of set hook InvalidSetter::$value must not be pass-by-reference"
    ));
}

#[test]
fn inherited_backing_property_rejects_reference_getter_with_setter() {
    let error = run_php_expect_error(
        r#"<?php
class BackingValue { public $value; }
class InvalidReferenceValue extends BackingValue {
    private $storage;
    public $value {
        &get => $this->storage;
        set => $this->storage = $value;
    }
}
"#,
    );
    assert!(format!("{error:?}").contains(
        "Get hook of backed property InvalidReferenceValue::value with set hook may not return by reference"
    ));
}

#[test]
fn value_getter_rejects_indirect_array_modification() {
    assert_eq!(
        run_php(
            r#"<?php
class BufferedList {
    public array $items {
        get { return $this->items; }
        set { $this->items = $value; }
    }
}
$list = new BufferedList();
$list->items = [];
try {
    $list->items[] = 7;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
var_dump($list->items);
"#,
        ),
        "Indirect modification of BufferedList::$items is not allowed\narray(0) {\n}\n"
    );
}

#[test]
fn foreach_by_reference_uses_reference_getters_and_rejects_value_getters() {
    assert_eq!(
        run_php(
            r#"<?php
class IteratedState {
    private int $storage = 9;
    public int $alias { &get => $this->storage; }
    public int $copy = 11 { get => $this->copy; }
}
$object = new IteratedState();
try {
    foreach ($object as $name => &$value) {
        echo "$name=$value\n";
    }
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        "alias=9\nCannot create reference to property IteratedState::$copy\n"
    );
}

#[test]
fn foreach_reference_preserves_typed_property_constraints() {
    assert_eq!(
        run_php(
            r#"<?php
class TypedIteration {
    public int $number = 4;
}
$object = new TypedIteration();
foreach ($object as &$value) {
    try {
        $value = 'invalid';
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
var_dump($object->number);
"#,
        ),
        "Cannot assign string to reference held by property TypedIteration::$number of type int\nint(4)\n"
    );
}

#[test]
fn property_hook_modifiers_are_rejected_at_declaration_time() {
    for (modifier, expected) in [
        (
            "public",
            "Cannot use the public modifier on a property hook",
        ),
        (
            "protected",
            "Cannot use the protected modifier on a property hook",
        ),
        (
            "private",
            "Cannot use the private modifier on a property hook",
        ),
        (
            "static",
            "Cannot use the static modifier on a property hook",
        ),
    ] {
        let error = run_php_expect_error(&format!(
            "<?php class InvalidModifier {{ public $value {{ {modifier} get {{}} }} }}"
        ));
        assert!(format!("{error:?}").contains(expected));
    }
}

#[test]
fn property_hook_names_must_be_known_and_unique() {
    let unknown =
        run_php_expect_error("<?php class UnknownHook { public $value { transform {} } }");
    let unknown = format!("{unknown:?}");
    assert!(unknown.contains("Unknown hook"));
    assert!(unknown.contains("transform"));
    assert!(unknown.contains("property UnknownHook::$value"));
    assert!(unknown.contains("expected"));

    let duplicate =
        run_php_expect_error("<?php class DuplicateHook { public $value { set {} SET {} } }");
    let duplicate = format!("{duplicate:?}");
    assert!(duplicate.contains("Cannot redeclare property hook"));
    assert!(duplicate.contains("set"));
}

#[test]
fn property_hook_list_must_not_be_empty() {
    let error = run_php_expect_error("<?php class EmptyHooks { public $value {} }");
    assert!(format!("{error:?}").contains("Property hook list must not be empty"));
}

#[test]
fn getter_parameter_lists_are_rejected_even_when_empty() {
    let error = run_php_expect_error("<?php class GetterArgs { public $value { get() {} } }");
    assert!(
        format!("{error:?}")
            .contains("get hook of property GetterArgs::$value must not have a parameter list")
    );
}

#[test]
fn setter_parameter_shape_is_validated_before_execution() {
    for declaration in ["set() {}", "set($first, $second) {}"] {
        let error = run_php_expect_error(&format!(
            "<?php class SetterArity {{ public $value {{ {declaration} }} }}"
        ));
        assert!(format!("{error:?}").contains(
            "set hook of property SetterArity::$value must accept exactly one parameters"
        ));
    }

    let defaulted = run_php_expect_error(
        "<?php class SetterDefault { public $value { set($incoming = 1) {} } }",
    );
    assert!(format!("{defaulted:?}").contains(
        "Parameter $incoming of set hook SetterDefault::$value must not have a default value"
    ));

    let variadic = run_php_expect_error(
        "<?php class SetterVariadic { public $value { set(...$incoming) {} } }",
    );
    assert!(
        format!("{variadic:?}").contains(
            "Parameter $incoming of set hook SetterVariadic::$value must not be variadic"
        )
    );
}

#[test]
fn promoted_property_assignment_runs_the_setter_before_constructor_body() {
    assert_eq!(
        run_php(
            r#"<?php
class PromotedTemperature {
    public function __construct(
        public int $celsius {
            get => $this->celsius;
            set => max(-273, $value);
        }
    ) {
        echo "body={$this->celsius}\n";
    }
}
$temperature = new PromotedTemperature(-500);
var_dump($temperature->celsius);
"#,
        ),
        "body=-273\nint(-273)\n"
    );
}

#[test]
fn promoted_hook_without_visibility_is_public_and_uses_its_setter() {
    assert_eq!(
        run_php(
            r#"<?php
class PromotedScale {
    public function __construct($amount = 4 { set => $value * 3; }) {}
}
$scale = new PromotedScale();
var_dump($scale);
"#,
        ),
        "object(PromotedScale)#1 (1) {\n  [\"amount\"]=>\n  int(12)\n}\n"
    );
}

#[test]
fn final_promoted_property_cannot_be_overridden() {
    let error = run_php_expect_error(
        r#"<?php
class FinalPromotion {
    public function __construct(final $value) {}
}
class InvalidChild extends FinalPromotion { public $value; }
"#,
    );
    assert!(format!("{error:?}").contains("Cannot override final property FinalPromotion::$value"));
}

#[test]
fn readonly_class_rejects_promoted_hooked_property() {
    let error = run_php_expect_error(
        r#"<?php
readonly class InvalidReadonlyPromotion {
    public function __construct(public int $value { set => $value; }) {}
}
"#,
    );
    assert!(format!("{error:?}").contains("Hooked properties cannot be readonly"));
}

#[test]
fn readonly_and_virtual_default_property_hooks_are_rejected() {
    let readonly = run_php_expect_error(
        r#"<?php
class ReadonlyHook {
    public readonly int $value { get; set; }
}
"#,
    );
    assert!(format!("{readonly:?}").contains("Hooked properties cannot be readonly"));

    let virtual_default = run_php_expect_error(
        r#"<?php
class VirtualDefault {
    public $value = 42 { get {} set {} }
}
"#,
    );
    assert!(format!("{virtual_default:?}").contains(
        "Cannot specify default value for virtual hooked property VirtualDefault::$value"
    ));

    let delayed_virtual_default = run_php_expect_error(
        r#"<?php
class NoInheritedStorage {}
class DelayedVirtualDefault extends NoInheritedStorage {
    public $value = 42 { get => 1; }
}
"#,
    );
    assert!(format!("{delayed_virtual_default:?}").contains(
        "Cannot specify default value for virtual hooked property DelayedVirtualDefault::$value"
    ));

    let static_parent = run_php_expect_error(
        r#"<?php
class StaticParentStorage { public static $value; }
class InvalidStorageKind extends StaticParentStorage {
    public $value = 42 { get => 1; }
}
"#,
    );
    assert!(format!("{static_parent:?}").contains(
        "Cannot redeclare static StaticParentStorage::$value as non static InvalidStorageKind::$value"
    ));
}

#[test]
fn inherited_storage_backs_a_child_property_hook_default() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentStorage { public $value; }
class InheritedStorageHook extends ParentStorage {
    public $value = 42 { get => 7; }
}
$property = new ReflectionProperty(InheritedStorageHook::class, 'value');
var_dump($property->isVirtual(), $property->hasDefaultValue(), $property->getDefaultValue());
"#,
        ),
        "bool(false)\nbool(true)\nint(42)\n"
    );
}

#[test]
fn class_and_trait_hooked_property_conflicts_are_rejected() {
    let error = run_php_expect_error(
        r#"<?php
trait HookSource {
    public $value { get => 1; }
}
class HookConsumer {
    use HookSource;
    public $value;
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        concat!(
            "Fatal(\"HookConsumer and HookSource define the same hooked property ",
            "($value) in the composition of HookConsumer. Conflict resolution between ",
            "hooked properties is currently not supported. Class was composed\")"
        )
    );

    let traits = run_php_expect_error(
        r#"<?php
trait FirstHookSource { public $value { get => 1; } }
trait SecondHookSource { public $value; }
class TwoHookSources { use FirstHookSource, SecondHookSource; }
"#,
    );
    assert_eq!(
        format!("{traits:?}"),
        concat!(
            "Fatal(\"FirstHookSource and SecondHookSource define the same hooked property ",
            "($value) in the composition of TwoHookSources. Conflict resolution between ",
            "hooked properties is currently not supported. Class was composed\")"
        )
    );
}

#[test]
fn explicit_parent_property_hooks_dispatch_to_the_parent_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentHookBase {
    public int $value {
        get => 40;
        set { echo "parent-set:$value\n"; }
    }
}
class ParentHookChild extends ParentHookBase {
    public int $value {
        get => parent::$value::GET() + 2;
        set { parent::$value::set($value + 1); }
    }
}
$child = new ParentHookChild();
$child->value = 9;
var_dump($child->value);
"#,
        ),
        "parent-set:10\nint(42)\n"
    );
}

#[test]
fn parent_property_hook_calls_are_restricted_to_the_matching_hook() {
    let wrong_property = run_php_expect_error(
        r#"<?php
class WrongParentProperty {
    public $first { get => parent::$second::get(); }
}
"#,
    );
    assert!(
        format!("{wrong_property:?}")
            .contains("Must not use parent::$second::get() in a different property ($first)")
    );

    let outside_hook = run_php_expect_error(
        r#"<?php
class ParentHookOutside {
    public function read() { return parent::$value::get(); }
}
"#,
    );
    assert!(
        format!("{outside_hook:?}")
            .contains("Must not use parent::$value::get() outside a property hook")
    );
}

#[test]
fn parent_hook_identity_and_interface_set_contracts_fail_during_linking() {
    let callable = run_php_expect_error(
        r#"<?php
class CallableHookBase { public mixed $item; }
class CallableHookChild extends CallableHookBase {
    public mixed $item {
        set {
            $setter = parent::$item::set(...);
            $setter($value);
        }
    }
}
(new CallableHookChild)->item = 1;
"#,
    );
    assert!(
        format!("{callable:?}").contains("Cannot create Closure for parent property hook call")
    );

    let numeric_member = run_php_expect_error(
        r#"<?php
class NumericHookChild {
    public $named { get => parent::${7}::get(); }
}
"#,
    );
    assert!(
        format!("{numeric_member:?}")
            .contains("Must not use parent::$7::get() in a different property ($named)")
    );

    let setter_contract = run_php_expect_error(
        r#"<?php
interface WritableText {
    public string $value { set(int|string $incoming); }
}
class NarrowText implements WritableText {
    public string $value;
}
"#,
    );
    assert!(
        format!("{setter_contract:?}").contains(
            "Set type of NarrowText::$value must be supertype of string|int (as in interface WritableText)"
        )
    );
}

#[test]
fn parent_hook_calls_without_class_scope_and_writable_call_results_are_rejected() {
    let no_scope = run_php_expect_error("<?php parent::$value::get();");
    assert!(format!("{no_scope:?}").contains("when no class scope is active"));

    let writable_result = run_php_expect_error(
        r#"<?php
class ParentReadBase { public $value { get => 1; } }
class ParentReadChild extends ParentReadBase {
    public $value { get => ++parent::$value::get(); }
}
"#,
    );
    assert!(
        format!("{writable_result:?}").contains("Can't use method return value in write context")
    );
}

#[test]
fn explicit_parent_backed_hooks_bypass_child_redispatch() {
    assert_eq!(
        run_php(
            r#"<?php
class BackedParentHook {
    public int $value = 1 {
        get => $this->value;
        set { $this->value = $value; }
    }
}
class BackedChildHook extends BackedParentHook {
    public int $value {
        get => parent::$value::get() + 1;
        set { parent::$value::set($value + 1); }
    }
}
$object = new BackedChildHook();
$object->value = 40;
var_dump($object->value);
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn implicit_parent_property_accessors_use_backing_storage_and_exact_arity() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainParentStorage {
    public int $value;
}
class PlainChildHooks extends PlainParentStorage {
    public int $value {
        get => parent::$value::get();
        set { parent::$value::set($value + 1); }
    }
}
$object = new PlainChildHooks();
$object->value = 40;
var_dump($object->value);

class ExtraArgumentParent { public $value = 42; }
class ExtraArgumentChild extends ExtraArgumentParent {
    public $value {
        get {
            try { parent::$value::get(1); } catch (ArgumentCountError $e) {
                echo $e->getMessage(), "\n";
            }
            return parent::$value::get();
        }
        set {
            try { parent::$value::set($value, 2); } catch (ArgumentCountError $e) {
                echo $e->getMessage(), "\n";
            }
        }
    }
}
$extra = new ExtraArgumentChild();
$extra->value = 1;
var_dump($extra->value);
"#,
        ),
        concat!(
            "int(41)\n",
            "ExtraArgumentParent::$value::set() expects exactly 1 argument, 2 given\n",
            "ExtraArgumentParent::$value::get() expects exactly 0 arguments, 1 given\n",
            "NULL\n",
        )
    );
}

#[test]
fn implicit_parent_property_accessors_report_property_errors() {
    assert_eq!(
        run_php(
            r#"<?php
class MissingParentProperty {}
class MissingChildProperty extends MissingParentProperty {
    public $missing {
        get {
            try { return parent::$missing::get(); }
            catch (Error $e) { echo $e->getMessage(), "\n"; return null; }
        }
    }
}
(new MissingChildProperty())->missing;

class PrivateParentProperty { private $secret = 42; }
class PrivateChildProperty extends PrivateParentProperty {
    public $secret {
        get {
            try { return parent::$secret::get(); }
            catch (Error $e) { echo $e->getMessage(), "\n"; return null; }
        }
    }
}
(new PrivateChildProperty())->secret;

class NoParentProperty {
    public $value {
        get {
            try { return parent::$value::get(); }
            catch (Error $e) { echo $e->getMessage(), "\n"; return null; }
        }
    }
}
(new NoParentProperty())->value;
"#,
        ),
        concat!(
            "Undefined property MissingParentProperty::$missing\n",
            "Cannot access private property PrivateParentProperty::$secret\n",
            "Cannot use \"parent\" when current class scope has no parent\n",
        )
    );
}

#[test]
fn parenthesized_parent_static_property_calls_the_fetched_class_name() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentClassNameStorage { public static $class = 'FetchedClassName'; }
class ChildClassNameStorage extends ParentClassNameStorage {
    public static function read() { return (parent::$class)::answer(); }
}
class FetchedClassName {
    public static function answer() { return 42; }
}
var_dump(ChildClassNameStorage::read());
"#,
        ),
        "int(42)\n"
    );
}

#[test]
fn hooked_properties_cannot_be_unset_and_keep_their_backing_value() {
    assert_eq!(
        run_php(
            r#"<?php
class HookedUnset {
    public int $backed {
        get => $this->backed;
        set { $this->backed = $value; }
    }
    public $virtual { get => 42; }
}
$object = new HookedUnset();
$object->backed = 41;
foreach (['backed', 'virtual'] as $property) {
    try { unset($object->{$property}); }
    catch (Error $error) { echo $error->getMessage(), "\n"; }
}
var_dump($object->backed, $object->virtual);
"#,
        ),
        concat!(
            "Cannot unset hooked property HookedUnset::$backed\n",
            "Cannot unset hooked property HookedUnset::$virtual\n",
            "int(41)\n",
            "int(42)\n",
        )
    );
}

#[test]
fn property_hook_method_names_are_hidden_from_direct_object_calls() {
    assert_eq!(
        run_php(
            r#"<?php
class HiddenHookMethods {
    public $value {
        get { echo "getter ran\n"; return 42; }
        set { echo "setter ran\n"; }
    }
}
$object = new HiddenHookMethods();
foreach (['$value::get', '$value::set'] as $method) {
    try { $object->{$method}(1); }
    catch (Error $error) { echo $error->getMessage(), "\n"; }
}

class MagicHiddenHookMethods {
    public $value { get => 42; }
    public function __call($method, $arguments) {
        echo "magic:$method:", count($arguments), "\n";
    }
}
(new MagicHiddenHookMethods())->{'$value::get'}(1, 2);
"#,
        ),
        concat!(
            "Call to undefined method HiddenHookMethods::$value::get()\n",
            "Call to undefined method HiddenHookMethods::$value::set()\n",
            "magic:$value::get:2\n",
        )
    );
}

#[test]
fn sensitive_parameter_redacts_property_setter_hook_arguments() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class SensitiveSetter {
    public mixed $value {
        set(#[SensitiveParameter] mixed $value) {
            throw new Exception('hook');
        }
    }
}
$object = new SensitiveSetter();
try {
    $object->value = 'concealed';
} catch (Exception $exception) {
    $frame = $exception->getTrace()[0];
    $argument = $frame['args'][0];
    echo $frame['class'], ':', $frame['type'], ':', $frame['function'], ':';
    echo get_class($argument), ':', $argument->getValue();
}
"#,
            "/app/sensitive-hook.php",
            "/app",
        ),
        "SensitiveSetter:->:$value::set:SensitiveParameterValue:concealed"
    );
}

#[test]
fn setter_hook_keeps_a_global_reference_across_the_detached_call_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class SharedHookStorage {
    public int $value {
        get => $this->value;
        set {
            global $shared;
            $this->value =& $shared;
        }
    }
}

$shared = 4;
$object = new SharedHookStorage;
$object->value = 99;
$shared++;
var_dump($object->value, $shared);
"#,
        ),
        "int(5)\nint(5)\n"
    );
}
