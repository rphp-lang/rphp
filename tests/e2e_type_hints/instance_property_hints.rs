#[test]
fn typed_instance_properties_are_uninitialized_per_object_after_warm_reads() {
    assert_eq!(
        run_php(
            r#"<?php
class InstanceInitialization {
    public int $number;
    public mixed $anything;
}

$first = new InstanceInitialization();
$first->number = 7;
for ($i = 0; $i < 8; $i++) { echo $first->number; }
$second = new InstanceInitialization();
try { echo $second->number; } catch (Error $error) { echo ":number:"; }
try { echo $second->anything; } catch (Error $error) { echo "mixed"; }
"#,
        ),
        "77777777:number:mixed"
    );
}

#[test]
fn direct_typed_property_getter_preserves_uninitialized_error() {
    assert_eq!(
        run_php(
            r#"<?php
class InstanceGetter {
    public int $number;
    public function getNumber(): int { return $this->number; }
}

$initialized = new InstanceGetter();
$initialized->number = 3;
for ($i = 0; $i < 100; $i++) { $initialized->getNumber(); }
$fresh = new InstanceGetter();
try { $fresh->getNumber(); } catch (TypeError $error) { echo "wrong"; }
catch (Error $error) { echo "uninitialized"; }
"#,
        ),
        "uninitialized"
    );
}

#[test]
fn weak_typed_instance_writes_coerce_and_recheck_warm_sites() {
    assert_eq!(
        run_php(
            r#"<?php
class WeakInstanceValues {
    public int $number;
    public float $ratio;
    public string $label;
    public bool $enabled;
}

$value = new WeakInstanceValues();
$value->number = "40";
$value->ratio = 2;
$value->label = 42;
$value->enabled = 0;
for ($i = 0; $i < 8; $i++) { $value->number = $i; }
try { $value->number = []; } catch (TypeError $error) { echo "type:"; }
echo $value->number . ":" . $value->ratio . ":" . $value->label . ":";
var_dump($value->enabled);
"#,
        ),
        "type:7:2:42:bool(false)\n"
    );
}

#[test]
fn strict_typed_instance_writes_fail_before_mutation_but_widen_int_to_float() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
class StrictInstanceValues {
    public int $number = 7;
    public float $ratio;
}

$value = new StrictInstanceValues();
for ($i = 0; $i < 4; $i++) { $value->number = $i; }
try { $value->number = "8"; } catch (TypeError $error) { echo "type:"; }
$value->ratio = 2;
echo $value->number . ":" . $value->ratio;
"#,
        ),
        "type:3:2"
    );
}

#[test]
fn typed_instance_object_union_nullable_and_inherited_scope_are_checked() {
    assert_eq!(
        run_php(
            r#"<?php
class InstanceNode {
    public self $current;
    public self|null $optional = null;
}
class InstanceNodeChild extends InstanceNode {}
class InstanceOther {}

$value = new InstanceNodeChild();
$value->current = new InstanceNodeChild();
$value->optional = new InstanceNode();
try { $value->current = new InstanceOther(); } catch (TypeError $error) { echo "type:"; }
echo get_class($value->current) . ":" . get_class($value->optional);
"#,
        ),
        "type:InstanceNodeChild:InstanceNode"
    );
}

#[test]
fn parenthesized_intersections_support_dnf_property_and_parameter_types() {
    assert_eq!(
        run_php(
            r#"<?php
interface DnfLeft {}
interface DnfRight {}
class DnfBoth implements DnfLeft, DnfRight {}
class DnfHolder {
    public (DnfLeft&DnfRight)|null $value = null;
    public function set((DnfLeft&DnfRight)|null $value): void { $this->value = $value; }
}
$holder = new DnfHolder();
$holder->set(new DnfBoth());
echo get_class($holder->value);
$holder->set(null);
echo $holder->value === null ? ':null' : ':wrong';
"#,
        ),
        "DnfBoth:null"
    );
}

#[test]
fn typed_instance_assignment_reads_reference_value_without_binding_property() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferencedInstanceValue { public int $number; }
function assignReferencedValue(ReferencedInstanceValue $value, &$source): void {
    $value->number = $source;
}
$source = 4;
$value = new ReferencedInstanceValue();
assignReferencedValue($value, $source);
$source = 9;
echo $value->number;
"#,
        ),
        "4"
    );
}

#[test]
fn promoted_typed_instance_properties_use_the_same_write_contract() {
    assert_eq!(
        run_php(
            r#"<?php
class PromotedInstanceValue {
    public function __construct(public int $number, public string $label) {}
}
$value = new PromotedInstanceValue(4, "5");
$value->number = "6";
echo $value->number . ":" . $value->label;
"#,
        ),
        "6:5"
    );
}

#[test]
fn invalid_typed_instance_default_is_rejected_during_compilation() {
    let error = run_php_expect_error(
        r#"<?php
class InvalidTypedInstanceDefault {
    public int $number = "1";
}
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(
            "Cannot use default value for property InvalidTypedInstanceDefault::$number of type int"
        ),
        "{rendered}"
    );
}

#[test]
fn inherited_readonly_property_initialization_uses_protected_set_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class ReadonlyBase {
    public readonly int $number;
    public function initializeBase(int $number): void { $this->number = $number; }
}
class ReadonlyChild extends ReadonlyBase {
    public function initializeChild(int $number): void { $this->number = $number; }
}
$fromBase = new ReadonlyChild();
$fromBase->initializeBase(1);
$fromChild = new ReadonlyChild();
$fromChild->initializeChild(2);
echo $fromBase->number . ":" . $fromChild->number;
"#,
        ),
        "1:2"
    );
}

#[test]
fn trait_instance_property_pseudo_types_use_the_consuming_class_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class TraitPropertyParent {}
trait TraitPropertyLinks {
    public self $peer;
    public parent $ancestor;
}
class TraitPropertyBase extends TraitPropertyParent { use TraitPropertyLinks; }
class TraitPropertyChild extends TraitPropertyBase {}

$value = new TraitPropertyChild();
$value->peer = new TraitPropertyBase();
$value->ancestor = new TraitPropertyParent();
try { $value->peer = new TraitPropertyParent(); }
catch (TypeError $error) { echo "type:"; }
echo get_class($value->peer) . ":" . get_class($value->ancestor);
"#,
        ),
        "type:TraitPropertyBase:TraitPropertyParent"
    );
}
