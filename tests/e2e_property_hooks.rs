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
