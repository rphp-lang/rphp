mod common;

use common::{run_php, run_php_expect_error};

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
