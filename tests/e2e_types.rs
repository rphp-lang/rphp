/// E2E tests: isset, empty, unset, type casting, type checks.
mod common;
use common::{run_php, run_php_with_source_context};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::opcode::OpCode;

#[test]
fn strict_union_calls_reject_non_members_and_widen_int_to_float() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function nullableUnion(int|string|null $value) { return $value; }
function floatOrArray(float|array $value) { return $value; }
try { nullableUnion(1.5); } catch (TypeError $error) { echo strstr($error->getMessage(), ', called in', true), "\n"; }
var_dump(floatOrArray(42));
$closure = eval('return function(int|string|null $value) { return $value; };');
try { $closure(1.5); } catch (TypeError $error) { echo strstr($error->getMessage(), ', called in', true), "\n"; }
"#,
        ),
        "nullableUnion(): Argument #1 ($value) must be of type string|int|null, float given\nfloat(42)\n{closure:<main>(7) : eval()'d code:1}(): Argument #1 ($value) must be of type string|int|null, float given\n"
    );
}

#[test]
fn weak_union_calls_select_scalar_coercions_by_php_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
function number(int|float $value) { var_dump($value); }
function integerOrBool(int|bool $value) { var_dump($value); }
function integerOrString(int|string|null $value) { var_dump($value); }
number('42'); number('42.0');
integerOrBool(42.0); integerOrBool(INF);
integerOrString(INF);
"#,
        ),
        "int(42)\nfloat(42)\nint(42)\nbool(true)\nstring(3) \"INF\"\n"
    );
}

#[test]
fn undefined_local_rvalues_warn_at_each_read_but_silent_and_reference_contexts_do_not() {
    let file = "/virtual/undefined-rvalue-contract.php";
    let source = r#"<?php
function acceptSnapshot($value) { var_dump($value); }
function fillReference(&$value) { $value = 'filled'; }
var_dump($firstMissing);
var_dump(isset($silentMissing), empty($silentMissing), $silentMissing ?? 'fallback');
acceptSnapshot($argumentMissing);
fillReference($referenceMissing);
echo $referenceMissing, "\n";"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!(
            "\nWarning: Undefined variable $firstMissing in {file} on line 4\nNULL\nbool(false)\nbool(true)\nstring(8) \"fallback\"\n\nWarning: Undefined variable $argumentMissing in {file} on line 6\nNULL\nfilled\n"
        )
    );
}

#[test]
fn runtime_resolved_calls_distinguish_reference_lvalues_from_value_reads() {
    let file = "/virtual/undefined-runtime-reference.php";
    let source = r#"<?php
class Relay {
    function bind(&$slot) { $slot = 'bound'; }
    function inspect($slot) { var_dump($slot); }
    static function bindStatic(&$slot) { $slot = 'static'; }
}
$relay = new Relay;
$relay->bind($methodMissing);
var_dump($methodMissing);
$dynamic = 'bind';
$relay->$dynamic($dynamicMissing);
var_dump($dynamicMissing);
$relay->bind(slot: $namedMissing);
var_dump($namedMissing);
Relay::bindStatic($staticMissing);
var_dump($staticMissing);
$relay->inspect(slot: $valueMissing);"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!(
            "string(5) \"bound\"\nstring(5) \"bound\"\nstring(5) \"bound\"\nstring(6) \"static\"\n\nWarning: Undefined variable $valueMissing in {file} on line 17\nNULL\n"
        )
    );
}

#[test]
fn runtime_resolved_variadics_and_suppression_keep_the_same_read_context() {
    let file = "/virtual/undefined-runtime-variadic.php";
    let source = r#"<?php
function bindAll(&...$slots) { foreach ($slots as &$slot) { $slot = 'set'; } }
function inspectAll(...$slots) { var_dump($slots); }
bindAll(first: $referenceMissing);
var_dump($referenceMissing);
function observeSuppressed($level, $message, $file, $line) { echo error_reporting(), ":$message:$line\n"; }
set_error_handler('observeSuppressed');
@inspectAll(first: $valueMissing);"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        "string(3) \"set\"\n4437:Undefined variable $valueMissing:8\narray(1) {\n  [\"first\"]=>\n  NULL\n}\n"
    );
}

#[test]
fn undefined_local_warning_uses_php_handler_return_reentrancy_and_suppression_rules() {
    let file = "/virtual/undefined-handler-contract.php";
    let source = r#"<?php
set_error_handler(function ($level, $message, $file, $line) { echo "claimed:$level:$message:$line\n"; $GLOBALS['handlerAssigned'] = 41; });
var_dump($handlerAssigned);
var_dump($handlerAssigned);
function declineMissing($level, $message, $file, $line) { echo "declined:$message\n"; return false; }
set_error_handler('declineMissing');
var_dump($declinedMissing);
function nestMissing($level, $message, $file, $line) { echo "outer:$message\n"; var_dump($nestedMissing); }
set_error_handler('nestMissing');
var_dump($outerMissing);
function inspectSuppression($level, $message, $file, $line) { echo 'suppressed:', error_reporting(), ":$message\n"; }
set_error_handler('inspectSuppression');
@var_dump($suppressedMissing);"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!(
            "claimed:2:Undefined variable $handlerAssigned:3\nNULL\nint(41)\ndeclined:Undefined variable $declinedMissing\n\nWarning: Undefined variable $declinedMissing in {file} on line 7\nNULL\nouter:Undefined variable $outerMissing\n\nWarning: Undefined variable $nestedMissing in {file} on line 8\nNULL\nNULL\nsuppressed:4437:Undefined variable $suppressedMissing\nNULL\n"
        )
    );
}

#[test]
fn explicit_closure_capture_snapshots_an_undefined_value_but_reference_capture_is_silent() {
    let file = "/virtual/undefined-capture-contract.php";
    let source = r#"<?php
$snapshot = function () use ($capturedMissing) { var_dump($capturedMissing); };
$snapshot();
$alias = function () use (&$referenceMissing) { $referenceMissing = 'bound'; };
$alias();
echo $referenceMissing;"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!("\nWarning: Undefined variable $capturedMissing in {file} on line 2\nNULL\nbound")
    );
}

#[test]
fn arrow_capture_keeps_an_undefined_snapshot_silent_until_the_body_reads_it() {
    let file = "/virtual/undefined-arrow-capture.php";
    let source = r#"<?php
$readLater = fn() => $notCreatedYet;
echo "closure-created\n";
$notCreatedYet = 9;
var_dump($readLater());
$ready = 5;
$readReady = fn() => $ready;
var_dump($readReady());"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!(
            "closure-created\n\nWarning: Undefined variable $notCreatedYet in {file} on line 2\nNULL\nint(5)\n"
        )
    );
}

#[test]
fn increment_initializes_an_undefined_local_after_reporting_the_read() {
    let file = "/virtual/undefined-increment.php";
    let source = r#"<?php
function advanceMissing() {
    var_dump($counter++);
    var_dump($counter);
    unset($counter);
    var_dump(++$counter);
}
advanceMissing();"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!(
            "\nWarning: Undefined variable $counter in {file} on line 3\nNULL\nint(1)\n\nWarning: Undefined variable $counter in {file} on line 6\nint(1)\n"
        )
    );
}

#[test]
fn increment_and_decrement_consume_the_pre_handler_undefined_snapshot() {
    let source = r#"<?php
set_error_handler(function ($level, $message, $file, $line) {
    echo "handled\n";
    $GLOBALS['step'] = 50;
});
var_dump($step++);
var_dump($step);
unset($step);
var_dump($step--);
var_dump($step);"#;

    assert_eq!(
        run_php(source),
        "handled\nNULL\nint(1)\nhandled\nNULL\nNULL\n"
    );
}

#[test]
fn undefined_local_after_a_partial_branch_keeps_its_runtime_warning() {
    let file = "/virtual/undefined-branch-contract.php";
    let source = r#"<?php
$takeBranch = false;
if ($takeBranch) { $branchOnly = 7; }
var_dump($branchOnly);
$takeBranch = true;
if ($takeBranch) { $both = 1; } else { $both = 2; }
var_dump($both);"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!("\nWarning: Undefined variable $branchOnly in {file} on line 4\nNULL\nint(1)\n")
    );
}

#[test]
fn compound_assignment_reads_a_main_scope_cv_after_rhs_reentrancy() {
    let file = "/virtual/compound-reentrant-read.php";
    let source = r#"<?php
function replaceTotal() { unset($GLOBALS['total']); return 2; }
$total = 5;
$total += replaceTotal();
var_dump($total);"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!("\nWarning: Undefined variable $total in {file} on line 4\nint(2)\n")
    );
}

#[test]
fn null_and_boolean_integer_arithmetic_preserves_php_result_kind() {
    assert_eq!(
        run_php("<?php var_dump(null + 2, false + 2, true + 2, null + 1.5);"),
        "int(2)\nint(2)\nint(3)\nfloat(1.5)\n"
    );
}

#[test]
fn global_reference_assignment_updates_a_reference_that_escaped_through_a_call() {
    assert_eq!(
        run_php(
            "<?php $published = null; function publish(&$slot) { $GLOBALS['published'] =& $slot; } function overwrite() { $GLOBALS['published'] = 9; } function observe() { $local = 1; publish($local); overwrite(); var_dump($local, $GLOBALS['published']); } observe();"
        ),
        "int(9)\nint(9)\n"
    );
}

#[test]
fn acquiring_a_reference_materializes_an_undefined_variable_as_null() {
    assert_eq!(
        run_php(
            "<?php function observeReference(&$slot) {} observeReference($callCreated); $closure = function () use (&$captureCreated) {}; $entries = [&$arrayCreated]; var_dump($callCreated, $captureCreated, $arrayCreated, $entries[0]);"
        ),
        "NULL\nNULL\nNULL\nNULL\n"
    );
}

#[test]
fn local_reference_aliases_share_one_cell_until_the_name_is_unset() {
    assert_eq!(
        run_php(
            r#"<?php
$left = "alpha";
$right =& $left;
$right = "beta";
var_dump($left, $right);
$left = "gamma";
var_dump($left, $right);
unset($right);
var_dump($left, isset($right));"#
        ),
        "string(4) \"beta\"\nstring(4) \"beta\"\nstring(5) \"gamma\"\nstring(5) \"gamma\"\nstring(5) \"gamma\"\nbool(false)\n"
    );
}

#[test]
fn local_reference_assignment_rebinds_an_existing_alias() {
    assert_eq!(
        run_php(
            "<?php $first = 1; $second = 2; $alias =& $first; $alias =& $second; $alias = 3; var_dump($first, $second, $alias);"
        ),
        "int(1)\nint(3)\nint(3)\n"
    );
}

#[test]
fn local_reference_binding_materializes_missing_and_self_sources() {
    assert_eq!(
        run_php(
            "<?php $missingAlias =& $missing; var_dump($missingAlias, $missing); $missingAlias = 7; var_dump($missing); $value = 1; $value =& $value; $value = 2; var_dump($value);"
        ),
        "NULL\nNULL\nint(7)\nint(2)\n"
    );
}

#[test]
fn local_reference_binding_is_an_lvalue_expression_and_accepts_this_as_a_source() {
    assert_eq!(
        run_php(
            r#"<?php
function mutate(&$slot) { $slot = 9; }
$value = 1;
mutate($alias =& $value);
var_dump($value, $alias);
class AliasThis {
    function inspect() {
        $alias =& $this;
        var_dump($alias === $this);
    }
}
(new AliasThis)->inspect();"#
        ),
        "int(9)\nint(9)\nbool(true)\n"
    );
}

#[test]
fn local_array_reference_mutation_preserves_an_ordinary_cow_copy() {
    assert_eq!(
        run_php(
            "<?php $original = [1]; $copy = $original; $alias =& $original; $alias[] = 2; var_dump($copy, $original, $alias);"
        ),
        "array(1) {\n  [0]=>\n  int(1)\n}\narray(2) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n}\narray(2) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n}\n"
    );
}

#[test]
fn rebinding_and_unsetting_the_last_alias_clear_array_reference_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2, 3];
$alias =& $values[0];
echo "FIRST\n";
var_dump($values);
$alias =& $values[1];
$alias = 9;
echo "SECOND\n";
var_dump($values);
unset($alias);
echo "LAST\n";
var_dump($values);"#
        ),
        "FIRST\narray(3) {\n  [0]=>\n  &int(1)\n  [1]=>\n  int(2)\n  [2]=>\n  int(3)\n}\nSECOND\narray(3) {\n  [0]=>\n  int(1)\n  [1]=>\n  &int(9)\n  [2]=>\n  int(3)\n}\nLAST\narray(3) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(9)\n  [2]=>\n  int(3)\n}\n"
    );
}

#[test]
fn array_element_reference_assignment_writes_back_nested_mutable_roots() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceBox {
    public array $values = [['score' => 1]];
}
$box = new ReferenceBox();
$propertyAlias =& $box->values[0]['score'];
$propertyAlias = 7;
$GLOBALS['state'] = [['score' => 2]];
$globalAlias =& $GLOBALS['state'][0]['score'];
$globalAlias = 8;
echo $box->values[0]['score'], ':', $GLOBALS['state'][0]['score'];"#
        ),
        "7:8"
    );
}

#[test]
fn nested_array_reference_cells_are_visible_to_var_dump() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 7;
$array = [&$value];
var_dump($array, $value);"#
        ),
        "array(1) {\n  [0]=>\n  &int(7)\n}\nint(7)\n"
    );
}

#[test]
fn object_property_reference_cells_are_visible_to_var_dump() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 11;
$object = new stdClass();
$object->property =& $value;
var_dump($object);"#
        ),
        "object(stdClass)#1 (1) {\n  [\"property\"]=>\n  &int(11)\n}\n"
    );
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceBox {
    public $property;
}
$value = 12;
$object = new ReferenceBox();
$object->property =& $value;
var_dump($object);"#
        ),
        "object(ReferenceBox)#1 (1) {\n  [\"property\"]=>\n  &int(12)\n}\n"
    );
}

#[test]
fn typed_instance_reference_constraints_coerce_reject_and_rebind() {
    assert_eq!(
        run_php(
            r#"<?php
class TypedInstanceReference {
    public int $number = 1;
    public ?int $nullable = 2;
    public int|string $wide = 3;
}
$object = new TypedInstanceReference;
$alias =& $object->number;
$alias = "4";
$object->nullable =& $alias;
$object->wide =& $alias;
echo $alias, ':', $object->number, ':', $object->nullable, ':', $object->wide, "\n";
try {
    $object->wide = "invalid";
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
$other = null;
$object->nullable =& $other;
$other = null;
var_dump($object->nullable, $object->number);
"#
        ),
        "4:4:4:4\nCannot assign string to reference held by property TypedInstanceReference::$number of type int\nNULL\nint(4)\n"
    );
}

#[test]
fn typed_reference_array_assignment_expression_returns_the_coerced_value() {
    assert_eq!(
        run_php(
            r#"<?php
class CoercedReferenceResult { public ?string $value; }
$object = new CoercedReferenceResult;
$direct =& $object->value;
var_dump($direct = 0, $object->value);
$array = [];
$array['slot'] =& $object->value;
$rhs = 12;
var_dump($array['slot'] = $rhs, $rhs, $object->value);
$nested = ['inner' => []];
$nested['inner'][0] =& $object->value;
var_dump($nested['inner'][0] = 34, $object->value);
"#
        ),
        "string(1) \"0\"\nstring(1) \"0\"\nstring(2) \"12\"\nint(12)\nstring(2) \"12\"\nstring(2) \"34\"\nstring(2) \"34\"\n"
    );
}

#[test]
fn dynamic_instance_reference_intersections_do_not_reuse_a_stale_property_slot() {
    assert_eq!(
        run_php(
            r#"<?php
class DynamicTypedReferences {
    public int $integer;
    public float $decimal;
    public ?int $nullableInteger;
    public ?string $nullableString;
}
function bindPair(DynamicTypedReferences $object, string $left, string $right, $value): void {
    try {
        $object->$right = $value;
        $object->$left =& $object->$right;
        echo "$left/$right:ok\n";
    } catch (TypeError $error) {
        echo "$left/$right:error\n";
    }
}
$object = new DynamicTypedReferences;
bindPair($object, 'integer', 'decimal', 42.0);
bindPair($object, 'integer', 'nullableInteger', 42);
bindPair($object, 'nullableInteger', 'nullableString', null);
echo $object->integer, ':';
var_dump($object->nullableInteger, $object->nullableString);
"#
        ),
        "integer/decimal:error\ninteger/nullableInteger:ok\nnullableInteger/nullableString:ok\n42:NULL\nNULL\n"
    );
}

#[test]
fn typed_instance_reference_owners_follow_uninitialized_clone_and_destruction_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
class RequiredInstanceReference {
    public int $required;
    public ?int $optional;
}
$uninitialized = new RequiredInstanceReference;
try {
    $bad =& $uninitialized->required;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
$ok =& $uninitialized->optional;
var_dump($ok);

class ClonedInstanceReference { public int $value = 1; }
$object = new ClonedInstanceReference;
$alias =& $object->value;
$clone = clone $object;
unset($object);
try {
    $alias = "blocked";
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
unset($clone);
$alias = "free";
var_dump($alias);
"#
        ),
        "Cannot access uninitialized non-nullable property RequiredInstanceReference::$required by reference\nNULL\nCannot assign string to reference held by property ClonedInstanceReference::$value of type int\nstring(4) \"free\"\n"
    );
}

#[test]
fn typed_property_errors_name_the_concrete_assigned_object_class() {
    assert_eq!(
        run_php(
            r#"<?php
interface LeftType {}
interface RightType {}
class BothTypes implements LeftType, RightType {}
class LeftOnly implements LeftType {}
class ObjectTypeSink {
    public string $scalar;
    public LeftType&RightType $intersection;
}

$sink = new ObjectTypeSink;
foreach ([new LeftOnly, new class {}] as $value) {
    try {
        $sink->scalar = $value;
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}

$shared = new BothTypes;
$sink->intersection =& $shared;
try {
    $shared = new LeftOnly;
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
"#
        ),
        "Cannot assign LeftOnly to property ObjectTypeSink::$scalar of type string\nCannot assign class@anonymous to property ObjectTypeSink::$scalar of type string\nCannot assign LeftOnly to reference held by property ObjectTypeSink::$intersection of type LeftType&RightType\n"
    );
}

#[test]
fn static_property_references_share_storage_and_initialize_nullable_slots() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticRefBase {
    public static $value = 1;
    public static ?int $nullable;
}
class StaticRefChild extends StaticRefBase {}
$class = StaticRefChild::class;
$name = "value";
$first =& $class::$$name;
$first = 2;
echo StaticRefBase::$value, "|";
$source = 3;
$class::$$name =& $source;
$source = 4;
echo StaticRefChild::$value, "|";
$fixed = 6;
StaticRefChild::$value =& $fixed;
$fixed = 7;
echo StaticRefBase::$value, "|";
$array = [&StaticRefBase::$value];
$array[0] = 8;
echo StaticRefChild::$value, "|";
$nullable =& StaticRefChild::$nullable;
var_dump($nullable);
$nullable = 5;
echo StaticRefBase::$nullable;
"#,
        ),
        "2|4|7|8|NULL\n5"
    );
}

#[test]
fn non_nullable_static_property_cannot_be_acquired_by_reference_before_initialization() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticNonNullableReference {
    public static int $value;
}
try {
    $value =& StaticNonNullableReference::$value;
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        "Cannot access uninitialized non-nullable property StaticNonNullableReference::$value by reference"
    );
}

#[test]
fn typed_static_reference_assignment_from_a_call_preserves_failed_initialization() {
    assert_eq!(
        run_php(
            r#"<?php
function &staticStringReference() {
    static $value = "invalid";
    return $value;
}
class StaticCallReference {
    public static int $typed;
}
try {
    StaticCallReference::$typed =& staticStringReference();
} catch (TypeError $error) {
    echo $error->getMessage(), "|";
}
try {
    var_dump(StaticCallReference::$typed);
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        "Cannot assign string to property StaticCallReference::$typed of type int|Typed static property StaticCallReference::$typed must not be accessed before initialization"
    );
}

#[test]
fn typed_static_reference_constraints_coerce_reject_and_detach_on_rebind() {
    assert_eq!(
        run_php(
            r#"<?php
class ConstrainedStaticReference {
    public static int $number = 1;
    public static $loose = 2;
}
$alias =& ConstrainedStaticReference::$number;
$alias = "3";
var_dump($alias, ConstrainedStaticReference::$number);
try {
    $alias = null;
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
ConstrainedStaticReference::$number =& ConstrainedStaticReference::$loose;
$alias = "detached";
var_dump($alias, ConstrainedStaticReference::$number);
"#,
        ),
        "int(3)\nint(3)\nCannot assign null to reference held by property ConstrainedStaticReference::$number of type int\nstring(8) \"detached\"\nint(2)\n"
    );
}

#[test]
fn one_reference_cell_enforces_the_intersection_of_compatible_property_types() {
    assert_eq!(
        run_php(
            r#"<?php
class CompatibleStaticReferences {
    public static int $exact = 1;
    public static ?int $nullable = 2;
    public static int|string $union = 3;
}
$alias =& CompatibleStaticReferences::$exact;
CompatibleStaticReferences::$nullable =& $alias;
CompatibleStaticReferences::$union =& $alias;
$alias = "4";
var_dump($alias, CompatibleStaticReferences::$exact, CompatibleStaticReferences::$nullable, CompatibleStaticReferences::$union);
try {
    CompatibleStaticReferences::$union = "invalid";
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
var_dump($alias, CompatibleStaticReferences::$union);
"#,
        ),
        "int(4)\nint(4)\nint(4)\nint(4)\nCannot assign string to reference held by property CompatibleStaticReferences::$exact of type int\nint(4)\nint(4)\n"
    );
}

#[test]
fn incompatible_typed_properties_cannot_hold_the_same_reference_cell() {
    assert_eq!(
        run_php(
            r#"<?php
class IncompatibleStaticReferences {
    public static int $number = 5;
    public static string $text = "5";
}
try {
    IncompatibleStaticReferences::$number =& IncompatibleStaticReferences::$text;
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
try {
    IncompatibleStaticReferences::$text =& IncompatibleStaticReferences::$number;
} catch (TypeError $error) {
    echo $error->getMessage();
}
"#,
        ),
        "Reference with value of type string held by property IncompatibleStaticReferences::$text of type string is not compatible with property IncompatibleStaticReferences::$number of type int\nReference with value of type int held by property IncompatibleStaticReferences::$number of type int is not compatible with property IncompatibleStaticReferences::$text of type string"
    );
}

#[test]
fn typed_static_property_keeps_a_reference_returned_from_a_call() {
    assert_eq!(
        run_php(
            r#"<?php
$shared = 6;
function &sharedReference() {
    return $GLOBALS['shared'];
}
class CallBoundStaticReference {
    public static string $value = "initial";
}
CallBoundStaticReference::$value =& sharedReference();
CallBoundStaticReference::$value = 7;
var_dump(CallBoundStaticReference::$value, sharedReference());
"#,
        ),
        "string(1) \"7\"\nstring(1) \"7\"\n"
    );
}

#[test]
fn typed_reference_constraints_follow_compound_container_and_global_alias_writes() {
    assert_eq!(
        run_php(
            r#"<?php
class EscapedTypedReference {
    public static int $value = 1;
}
$alias =& EscapedTypedReference::$value;
try {
    $alias .= "invalid";
} catch (TypeError $error) {
    echo "compound:", $error->getMessage(), "\n";
}
$alias++;
$container = [&$alias];
try {
    $container[0] = "invalid";
} catch (TypeError $error) {
    echo "container:", $error->getMessage(), "\n";
}
$GLOBALS['escapedAlias'] =& $alias;
try {
    $GLOBALS['escapedAlias'] = "invalid";
} catch (TypeError $error) {
    echo "global:", $error->getMessage(), "\n";
}
var_dump($alias, EscapedTypedReference::$value);
"#,
        ),
        "compound:Cannot assign string to reference held by property EscapedTypedReference::$value of type int\ncontainer:Cannot assign string to reference held by property EscapedTypedReference::$value of type int\nglobal:Cannot assign string to reference held by property EscapedTypedReference::$value of type int\nint(2)\nint(2)\n"
    );
}

#[test]
fn compiler_reference_cvs_do_not_create_visible_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
$source = [1];
$destination = [];
$destination[0] =& $source[0];
unset($source);
$holder = (object) ['property' => 2];
$destination[1] =& $holder->property;
unset($holder->property);
$property = 'property';
$dynamicHolder = (object) ['property' => 3];
$destination[2] =& $dynamicHolder->{$property};
unset($dynamicHolder->{$property});
$variableName = 'dynamicSource';
$dynamicSource = 4;
$destination[3] =& $$variableName;
unset($dynamicSource);
var_dump($destination);"#
        ),
        "array(4) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n  [2]=>\n  int(3)\n  [3]=>\n  int(4)\n}\n"
    );
    assert_eq!(
        run_php(
            r#"<?php
$value = 13;
$object = new stdClass();
$object->property =& $value;
unset($value);
var_dump($object);"#
        ),
        "object(stdClass)#1 (1) {\n  [\"property\"]=>\n  int(13)\n}\n"
    );
}

#[test]
fn destructuring_reads_a_retained_rhs_while_writing_through_an_alias() {
    assert_eq!(
        run_php(
            "<?php $source = [11, 22]; $mirror =& $source; [$mirror, $tail] = $source; var_dump($source, $tail);"
        ),
        "int(11)\nint(22)\n"
    );
}

#[test]
fn definitely_initialized_function_locals_keep_compact_cv_operands() {
    let source = "<?php function accumulate($limit) { $sum = 0; for ($i = 0; $i < $limit; $i++) { $sum += $i; } if ($limit > 0) { $result = $sum; } else { $result = 0; } return $result; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let function = compiled
        .functions
        .iter()
        .find(|(name, _)| name == "accumulate")
        .map(|(_, function)| function)
        .unwrap();

    assert!(
        function
            .op_array
            .instructions()
            .iter()
            .all(|instruction| instruction.opcode != OpCode::FetchCvR),
        "proven initialized locals must not add diagnostic dispatch to the hot loop"
    );
}

#[test]
fn error_reporting_is_request_local_and_uses_namespaced_function_fallback() {
    assert_eq!(
        run_php(
            "<?php namespace App; echo error_reporting(), ':'; echo error_reporting(5), ':'; echo error_reporting();"
        ),
        "32767:32767:5"
    );
}

#[test]
fn error_suppression_follows_a_called_frame_and_restores_the_request_mask() {
    assert_eq!(
        run_php(
            "<?php error_reporting(E_ALL); function inspectMaskedWarning() { echo $forgotten; throw new RuntimeException('stop'); } try { @inspectMaskedWarning(); } catch (RuntimeException $error) {} echo 'mask=', error_reporting();"
        ),
        "mask=32767"
    );
}

#[test]
fn error_suppression_hides_builtin_undefined_variable_warnings_without_a_handler() {
    assert_eq!(
        run_php("<?php @$directMissing; @($groupedMissing + 1); echo 'ok';"),
        "ok"
    );
}

#[test]
fn reference_read_contexts_materialize_null_without_ordinary_read_warnings() {
    let source = r#"<?php
static $topLevel = -1;
global $topLevel;
var_dump($topLevel, $GLOBALS['topLevel']);
function publish() { global $freshGlobal; var_dump($freshGlobal); $freshGlobal += 2; }
publish();
var_dump($freshGlobal);
function &missingAlias() { return $returnSlot; }
var_dump(missingAlias());
function recurseArg($value) { recurseArg($value[][$silentKey]); }
try { recurseArg([]); } catch (Error $error) { echo $error->getMessage(), "\n"; }
function sink($first, &$alias) { $alias = 9; }
$lead = 1;
sink($lead, $missingRef, ...[]);
var_dump($missingRef);"#;

    assert_eq!(
        run_php(source),
        "int(-1)\nint(-1)\nNULL\nint(2)\nNULL\nCannot use [] for reading\nint(9)\n"
    );
}

#[test]
fn reference_returning_calls_preserve_aliases_through_forwarders_and_finally() {
    let source = r#"<?php
function &leaf(&$slot) { return $slot; }
function &forward(&$slot) { return leaf($slot); }
function &throughFinally(&$slot) { try { return forward($slot); } finally { echo "finally:"; } }
$value = 5;
$alias =& throughFinally($value);
$alias = 13;
echo $value, ':', $alias, "\n";
$closure = function &(&$slot) { return $slot; };
$closureAlias =& $closure($value);
$closureAlias = 21;
echo $value, ':', $alias, ':', $closureAlias, "\n";"#;

    assert_eq!(run_php(source), "finally:13:13\n21:21:21\n");
}

#[test]
fn static_reference_returns_survive_first_class_callable_and_pipe_forwarders() {
    let source = r#"<?php
function &staticSlot($suffix) {
    static $value = "original";
    $value .= " " . $suffix;
    return $value;
}
function &throughPipe() { return "pipe" |> staticSlot(...); }
$callable = staticSlot(...);
$direct =& $callable("callable");
$direct = "changed";
echo staticSlot("direct"), "\n";
$piped =& throughPipe();
$piped = "forwarded";
echo staticSlot("after"), "\n";"#;

    assert_eq!(run_php(source), "changed direct\nforwarded after\n");
}

#[test]
fn reference_returns_preserve_global_array_and_method_property_cells() {
    let source = r#"<?php
function &globalSlot() { return $GLOBALS['shared']; }
class Holder {
    public $value = 3;
    public function &slot() { return $this->value; }
}
$shared = 1;
$globalAlias =& globalSlot();
$globalAlias = 2;
$holder = new Holder;
$propertyAlias =& $holder->slot();
$propertyAlias = 4;
echo $shared, ':', $globalAlias, ':', $holder->value, ':', $propertyAlias, "\n";"#;

    assert_eq!(run_php(source), "2:2:4:4\n");
}

#[test]
fn invalid_reference_call_and_return_diagnostics_use_the_operator_source_line() {
    let file = "/virtual/reference-return-diagnostics.php";
    let source = r#"<?php
function value() { return 1; }
function &invalid() { return 2; }
$assigned =& value();
$returned =& invalid();"#;

    assert_eq!(
        run_php_with_source_context(source, file, "/virtual"),
        format!(
            "\nNotice: Only variables should be assigned by reference in {file} on line 4\n\nNotice: Only variable references should be returned by reference in {file} on line 3\n"
        )
    );
}

#[test]
fn by_value_reads_separate_reference_returning_call_results() {
    let source = r#"<?php
function &expose(&$slot) { return $slot; }
$value = 5;
$copy = expose($value);
$copy = 13;
echo $value, ':', $copy, "\n";"#;

    assert_eq!(run_php(source), "5:13\n");
}

#[test]
fn explicit_reporting_change_inside_suppressed_call_reenables_warning_and_persists() {
    let output = run_php(
        "<?php error_reporting(E_NOTICE); function revealMaskedWarning() { error_reporting(E_ALL); echo $forgotten; throw new RuntimeException('stop'); } try { @revealMaskedWarning(); } catch (RuntimeException $error) {} echo 'mask=', error_reporting();",
    );

    assert!(
        output.contains("Warning: Undefined variable $forgotten"),
        "{output}"
    );
    assert!(output.ends_with("mask=32767"), "{output}");
}

#[test]
fn error_log_default_destination_uses_namespaced_function_fallback() {
    assert_eq!(
        run_php("<?php namespace App; var_dump(error_log('rphp-test'));"),
        "bool(true)\n"
    );
}

#[test]
fn invokable_object_satisfies_callable_parameter_and_return_types() {
    assert_eq!(
        run_php(
            "<?php class Handler { public function __invoke(): string { return 'OK'; } } function keep(callable $handler): callable { return $handler; } echo keep(new Handler())();"
        ),
        "OK"
    );
}

#[test]
fn reflection_function_exposes_anonymous_and_bound_closure_identity() {
    assert_eq!(
        run_php(
            "<?php $anonymous = function () {}; $anonymousReflection = new ReflectionFunction($anonymous); var_dump($anonymousReflection->isAnonymous()); echo count($anonymousReflection->getAttributes()), ':'; class Bound { public function named() {} public function callback() { return $this->named(...); } } $bound = new Bound(); $reflection = new ReflectionFunction($bound->callback()); var_dump($reflection->isAnonymous()); echo ($reflection->getClosureThis() === $bound ? 'bound:' : 'missing:') . $reflection->getClosureCalledClass()->name;"
        ),
        "bool(true)\n0:bool(false)\nbound:Bound"
    );
}

#[test]
fn reflection_function_parameters_expose_controller_metadata_surface() {
    assert_eq!(
        run_php(
            "<?php function reflected(string $name, ?int $count = null, bool ...$flags) {} $parameters = (new ReflectionFunction('reflected'))->getParameters(); foreach ($parameters as $parameter) { $type = $parameter->getType(); echo $parameter->getName(), ':', $type?->getName(), ':', (int) $type?->isBuiltin(), ':', (int) $parameter->allowsNull(), ':', (int) $parameter->isDefaultValueAvailable(), ':', (int) $parameter->isVariadic(), '|'; }"
        ),
        "name:string:1:0:0:0|count:int:1:1:1:0|flags:bool:1:0:0:1|"
    );
}

#[test]
fn reflection_class_lazy_ghost_defers_initializer_until_property_access() {
    assert_eq!(
        run_php(
            "<?php class LazyService { public string $value; public function __construct(string $value) { $this->value = $value; } } $service = (new ReflectionClass(LazyService::class))->newLazyGhost(static function ($ghost) { echo 'initializer:'; $ghost->__construct('OK'); }); echo 'before:', $service->value;"
        ),
        "before:initializer:OK"
    );
}

#[test]
fn reflection_class_lazy_proxy_preserves_shell_identity_and_forwards_properties() {
    assert_eq!(
        run_php(
            "<?php class LazyProxyService { public int $value = 1; } $reflection = new ReflectionClass(LazyProxyService::class); $proxy = $reflection->newLazyProxy(static function ($shell) { echo $shell::class, ':factory:'; $real = new LazyProxyService(); $real->value = 4; return $real; }); $id = spl_object_id($proxy); echo (int) $reflection->isUninitializedLazyObject($proxy), ':'; echo $proxy->value, ':'; echo (int) ($id === spl_object_id($proxy)), ':', (int) $reflection->isUninitializedLazyObject($proxy);"
        ),
        "1:LazyProxyService:factory:4:1:0"
    );
}

#[test]
fn reflection_class_allows_lazy_stdclass_without_reclassifying_internal_types() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = 0;
$reflection = new ReflectionClass('STDCLASS');
foreach (['newLazyGhost', 'newLazyProxy'] as $method) {
    $object = $reflection->$method(function () use (&$calls) {
        ++$calls;
    });
    echo $method, ':', $object::class, ':';
    echo (int) $reflection->isInternal(), ':';
    echo (int) $reflection->isUninitializedLazyObject($object), "\n";
}
try {
    (new ReflectionClass(Exception::class))->newLazyGhost(function () {});
    echo "accepted\n";
} catch (Throwable) {
    echo "rejected\n";
}
echo "calls:$calls";
"#,
        ),
        concat!(
            "newLazyGhost:stdClass:1:0\n",
            "newLazyProxy:stdClass:1:0\n",
            "rejected\n",
            "calls:0",
        )
    );
}

#[test]
fn reflection_lazy_raw_writes_validate_atomically_and_skip_slots_independently() {
    assert_eq!(
        run_php(
            r#"<?php
class AtomicLazySlot {
    public int $number;
    public string $label = 'seed';
}
$reflection = new ReflectionClass(AtomicLazySlot::class);
$number = $reflection->getProperty('number');
$label = $reflection->getProperty('label');

$rejected = $reflection->newLazyGhost(function ($object) {
    ob_start();
    var_dump($object);
    $dump = ob_get_clean();
    echo str_starts_with($dump, 'object(AtomicLazySlot)') ? "ordinary\n" : "lazy\n";
    echo "initialize\n";
    $object->number = 41;
    $object->label = 'ready';
});
try {
    $number->setRawValueWithoutLazyInitialization($rejected, new stdClass());
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
echo (int) $reflection->isUninitializedLazyObject($rejected), ':', $rejected->number, ':', $rejected->label, "\n";

$partial = $reflection->newLazyGhost(function () { echo "unexpected\n"; });
$number->setRawValueWithoutLazyInitialization($partial, '7');
echo $partial->number, ':', (int) $reflection->isUninitializedLazyObject($partial), ':';
$label->skipLazyInitialization($partial);
echo (int) $reflection->isUninitializedLazyObject($partial), ':', $partial->label;
"#,
        ),
        "Cannot assign stdClass to property AtomicLazySlot::$number of type int\n1:ordinary\ninitialize\n41:ready\n7:1:0:seed"
    );
}

#[test]
fn lazy_initialization_transacts_typed_reference_sources() {
    assert_eq!(
        run_php(
            r#"<?php
class LazyReferenceTransaction {
    public ?self $kept;
    public ?self $replaced;
    public $trigger;

    public function initialize(bool $throws): void {
        $this->kept = null;
        unset($this->replaced);
        if ($throws) {
            throw new Exception('rollback');
        }
        $this->replaced = null;
    }
}

$reflection = new ReflectionClass(LazyReferenceTransaction::class);
$ghost = $reflection->newLazyGhost(fn ($object) => $object->initialize(false));
$reflection->getProperty('kept')->setRawValueWithoutLazyInitialization($ghost, null);
$ghostKept = &$ghost->kept;
$reflection->getProperty('replaced')->setRawValueWithoutLazyInitialization($ghost, null);
$ghostReplaced = &$ghost->replaced;
var_dump($ghost->trigger);
try {
    $ghostKept = 1;
} catch (TypeError) {
    echo "kept constrained\n";
}
unset($ghost->kept);
$ghostKept = 1;
$ghostReplaced = 2;
echo 'freed:', $ghostKept, ':', $ghostReplaced, "\n";

$failed = $reflection->newLazyGhost(fn ($object) => $object->initialize(true));
$reflection->getProperty('kept')->setRawValueWithoutLazyInitialization($failed, null);
$failedKept = &$failed->kept;
$reflection->getProperty('replaced')->setRawValueWithoutLazyInitialization($failed, null);
$failedReplaced = &$failed->replaced;
try {
    var_dump($failed->trigger);
} catch (Exception $exception) {
    echo $exception->getMessage(), "\n";
}
ob_start();
var_dump($failed);
$dump = ob_get_clean();
echo 'aliases:', substr_count($dump, '&NULL'), "\n";
try {
    $failedKept = 1;
} catch (TypeError) {
    echo "rollback kept\n";
}
try {
    $failedReplaced = 1;
} catch (TypeError) {
    echo "rollback replaced\n";
}

$proxy = $reflection->newLazyProxy(fn () => new LazyReferenceTransaction());
$reflection->getProperty('kept')->setRawValueWithoutLazyInitialization($proxy, null);
$proxyKept = &$proxy->kept;
$reflection->getProperty('replaced')->setRawValueWithoutLazyInitialization($proxy, null);
$proxyReplaced = &$proxy->replaced;
var_dump($proxy->trigger);
$proxyKept = 3;
$proxyReplaced = 4;
echo 'proxy:', $proxyKept, ':', $proxyReplaced, "\n";
"#,
        ),
        concat!(
            "NULL\n",
            "kept constrained\n",
            "freed:1:2\n",
            "rollback\n",
            "aliases:2\n",
            "rollback kept\n",
            "rollback replaced\n",
            "NULL\n",
            "proxy:3:4\n",
        )
    );
}

#[test]
fn reflection_lazy_raw_write_reaches_terminal_nested_proxy_instance() {
    assert_eq!(
        run_php(
            r#"<?php
class NestedLazySlot {
    public string $payload = 'initial';
}
$reflection = new ReflectionClass(NestedLazySlot::class);
$first = new NestedLazySlot();
$outer = $reflection->newLazyProxy(fn () => $first);
$reflection->initializeLazyObject($outer);
$reflection->resetAsLazyProxy($first, function () {
    $last = new NestedLazySlot();
    $last->payload = 'terminal';
    return $last;
});
$last = $reflection->initializeLazyObject($first);
$reflection->getProperty('payload')->setRawValueWithoutLazyInitialization($outer, 'written');
echo $outer->payload, ':', $first->payload, ':', $last->payload;
"#,
        ),
        "written:written:written"
    );
}

#[test]
fn reflection_lazy_reset_retires_selected_storage_before_reinitialization() {
    assert_eq!(
        run_php(
            r#"<?php
class ResetPayload {
    public function __destruct() { echo "release\n"; }
}
class ResetBase {
    public $base;
    public function __destruct() { echo 'owner:', gettype($this->base), ':', $this->child, "\n"; }
}
#[AllowDynamicProperties]
class ResetChild extends ResetBase {
    public string $child = 'keep';
}
$object = new ResetChild();
$object->base = new ResetPayload();
$object->dynamic = new ResetPayload();
$reflection = new ReflectionClass(ResetBase::class);
$reflection->resetAsLazyGhost($object, function ($object) {
    echo "initialize\n";
    $object->base = 'ready';
});
echo $object->child, ':', (int) $reflection->isUninitializedLazyObject($object), "\n";
echo $object->base, "\n";
$object = null;
"#,
        ),
        "owner:object:keep\nrelease\nrelease\nkeep:1\ninitialize\nready\nowner:string:keep\n"
    );
}

#[test]
fn reflection_lazy_reset_destructor_failure_keeps_the_original_lifecycle_retired() {
    assert_eq!(
        run_php(
            r#"<?php
class RejectedLazyReset {
    public string $value = 'original';
    public function __destruct() {
        echo "destructor\n";
        throw new Exception('stop');
    }
}
$object = new RejectedLazyReset();
$reflection = new ReflectionClass(RejectedLazyReset::class);
try {
    $reflection->resetAsLazyProxy($object, fn () => new RejectedLazyReset());
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
echo (int) $reflection->isUninitializedLazyObject($object), ':', $object->value, "\n";
$object = null;
"#,
        ),
        "destructor\nstop\n0:original\n"
    );
}

#[test]
fn lazy_serialization_hooks_observe_state_only_when_they_access_it() {
    assert_eq!(
        run_php(
            r#"<?php
class LazyWireValue {
    public int $value;
    private int $hidden = 7;
    public function __serialize(): array { return []; }
}
$reflection = new ReflectionClass(LazyWireValue::class);
$ghost = $reflection->newLazyGhost(function ($object) {
    echo "unexpected ghost initialization\n";
    $object->value = 1;
});
$proxy = $reflection->newLazyProxy(function () {
    echo "unexpected proxy initialization\n";
    return new LazyWireValue();
});
echo serialize($ghost), ':', (int) $reflection->isUninitializedLazyObject($ghost), "\n";
echo serialize($proxy), ':', (int) $reflection->isUninitializedLazyObject($proxy), "\n";

class SleepingLazyWireValue {
    public int $value;
    private int $hidden = 7;
    public function __sleep(): array {
        echo $this->value, "\n";
        return ['value', 'hidden'];
    }
}
$reflection = new ReflectionClass(SleepingLazyWireValue::class);
$proxy = $reflection->newLazyProxy(function () {
    $object = new SleepingLazyWireValue();
    $object->value = 3;
    return $object;
}, ReflectionClass::SKIP_INITIALIZATION_ON_SERIALIZE);
echo serialize($proxy), "\n";
"#,
        ),
        concat!(
            "O:13:\"LazyWireValue\":0:{}:1\n",
            "O:13:\"LazyWireValue\":0:{}:1\n",
            "3\n",
            "O:21:\"SleepingLazyWireValue\":2:{s:5:\"value\";i:3;",
            "s:29:\"\0SleepingLazyWireValue\0hidden\";i:7;}\n"
        )
    );
}

#[test]
fn lazy_object_enumeration_observes_backed_and_virtual_property_getters() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class LazyProjection {
    public int $base;
    public int $backed {
        get { return $this->backed; }
        set(int $value) { $this->backed = $value; }
    }
    public int $virtual {
        get { return $this->base + 2; }
    }
}
$reflection = new ReflectionClass(LazyProjection::class);
$object = $reflection->newLazyGhost(function ($object) {
    echo "initialize\n";
    $object->base = 1;
    $object->backed = 2;
    $object->dynamic = 4;
});
echo json_encode($object), "\n";
foreach ($object as $name => $value) {
    echo $name, ':', $value, "\n";
}
"#,
        ),
        concat!(
            "initialize\n",
            "{\"backed\":2,\"base\":1,\"dynamic\":4,\"virtual\":3}\n",
            "base:1\nbacked:2\nvirtual:3\ndynamic:4\n"
        )
    );
}

#[test]
fn lazy_callback_clones_and_nested_proxy_projections_preserve_semantics() {
    assert_eq!(
        run_php(
            r#"<?php
class CapturedLazyTarget {
    public function __construct(public int $value) {}
}
$reflection = new ReflectionClass(CapturedLazyTarget::class);
function nestedProxy($reflection, &$captured) {
    $lazy = $reflection->newLazyProxy(function () use (&$captured) {
        return $captured = new CapturedLazyTarget(1);
    });
    $reflection->initializeLazyObject($lazy);
    $reflection->resetAsLazyProxy($captured, function () {
        return new CapturedLazyTarget(3);
    });
    return $lazy;
}
$property = nestedProxy($reflection, $captured);
var_dump($captured instanceof CapturedLazyTarget, $captured === $property);
echo $property->value, "\n";
$json = nestedProxy($reflection, $captured);
echo json_encode($json), "\n";
$iterable = nestedProxy($reflection, $captured);
foreach ($iterable as $name => $value) {
    echo $name, ':', $value, "\n";
}
"#,
        ),
        concat!(
            "bool(true)\nbool(false)\n",
            "3\n",
            "{\"value\":3}\n",
            "value:3\n"
        )
    );
}

#[test]
fn lazy_proxy_magic_guards_follow_shell_and_real_instance_recursion() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class GuardedProxyTarget {
    public $_;
    public function &__get($name) {
        global $shell;
        echo "get:$name\n";
        return $shell->$name;
    }
    public function __isset($name) {
        global $shell;
        echo "isset:$name\n";
        return isset($shell->$name);
    }
    public function __set($name, $value) {
        global $shell;
        echo "set:$name\n";
        $shell->$name = $value;
    }
    public function __unset($name) {
        global $shell;
        echo "unset:$name\n";
        unset($shell->$name);
    }
}
set_error_handler(function ($code, $message) { echo "warning:$message\n"; return true; });
$reflection = new ReflectionClass(GuardedProxyTarget::class);
$shell = $reflection->newLazyProxy(fn () => new GuardedProxyTarget());
$real = $reflection->initializeLazyObject($shell);
$reference = &$real->missing;
var_dump($reference);
var_dump(isset($real->check));
$real->written = 7;
var_dump($real->written);
unset($real->gone);
"#,
        ),
        "get:missing\nwarning:Undefined property: GuardedProxyTarget::$missing\nNULL\nisset:check\nbool(false)\nset:written\nint(7)\nunset:gone\n"
    );
}

#[test]
fn lazy_proxy_magic_guard_survives_initialization_mid_access() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class MidAccessProxy {
    public $_;
    public function __isset($name) {
        echo "isset:$name\n";
        return isset($this->$name[0]);
    }
    public function __get($name) {
        echo "get:$name\n";
        return $this->$name[0];
    }
}
set_error_handler(function ($code, $message) { echo "warning:$message\n"; return true; });
$reflection = new ReflectionClass(MidAccessProxy::class);
$shell = $reflection->newLazyProxy(function () {
    echo "initialize\n";
    return new MidAccessProxy();
});
var_dump(isset($shell->slot[0]));
"#,
        ),
        "isset:slot\nget:slot\ninitialize\nwarning:Undefined property: MidAccessProxy::$slot\nwarning:Trying to access array offset on null\nbool(false)\n"
    );
}

#[test]
fn lazy_magic_property_access_stays_deferred_until_magic_observes_state() {
    assert_eq!(
        run_php(
            "<?php class LazyMagicService { public int $value = 1; public function __get($name) { return $name; } } $reflection = new ReflectionClass(LazyMagicService::class); $service = $reflection->newLazyGhost(static function ($shell) { echo 'initializer:'; $shell->value = 5; }); echo $service->missing, ':', $service->value;"
        ),
        "missing:initializer:5"
    );
}

#[test]
fn core_iterator_and_collection_interfaces_are_registered() {
    assert_eq!(
        run_php(
            "<?php echo interface_exists('IteratorAggregate', false) ? 'yes' : 'no'; echo ':'; echo interface_exists('ArrayAccess', false) ? 'yes' : 'no';"
        ),
        "yes:yes"
    );
}

#[test]
fn spl_object_storage_is_a_core_collection_type() {
    assert_eq!(
        run_php(
            "<?php $storage = new SplObjectStorage(); echo $storage instanceof Iterator ? 'iterator:' : 'missing:'; echo $storage instanceof ArrayAccess ? 'array-access' : 'missing';"
        ),
        "iterator:array-access"
    );
}

#[test]
fn array_iterator_participates_in_foreach() {
    assert_eq!(
        run_php(
            "<?php $iterator = new ArrayIterator(['first' => 1, 'second' => 2]); foreach ($iterator as $key => $value) echo $key . ':' . $value . '|';"
        ),
        "first:1|second:2|"
    );
}

#[test]
fn array_iterator_and_array_object_expose_distinct_traversal_contracts() {
    assert_eq!(
        run_php(
            "<?php $iterator = new ArrayIterator([]); $object = new ArrayObject([]); echo (int) ($iterator instanceof Iterator), (int) ($iterator instanceof IteratorAggregate), '|', (int) ($object instanceof Iterator), (int) ($object instanceof IteratorAggregate);"
        ),
        "10|01"
    );
}

#[test]
fn error_and_exception_handler_stacks_restore_previous_callbacks() {
    assert_eq!(
        run_php(
            "<?php set_error_handler('strlen'); echo get_error_handler(), ':'; set_error_handler(null); restore_error_handler(); echo get_error_handler(), ':'; set_exception_handler('trim'); echo get_exception_handler();"
        ),
        "strlen:strlen:trim"
    );
}

// ========== isset() ==========

#[test]
fn test_isset_defined_var() {
    assert_eq!(
        run_php("<?php $x = 42; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_undefined_var() {
    assert_eq!(run_php("<?php echo isset($x) ? 'yes' : 'no';"), "no");
}

#[test]
fn test_isset_null_var() {
    assert_eq!(
        run_php("<?php $x = null; echo isset($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_isset_zero_is_set() {
    assert_eq!(
        run_php("<?php $x = 0; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_empty_string_is_set() {
    assert_eq!(
        run_php("<?php $x = ''; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_false_is_set() {
    assert_eq!(
        run_php("<?php $x = false; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_multi_arg() {
    assert_eq!(
        run_php("<?php $a = 1; $b = 2; echo isset($a, $b) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_multi_arg_one_null() {
    assert_eq!(
        run_php("<?php $a = 1; $b = null; echo isset($a, $b) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_isset_rejects_expression() {
    let result =
        std::panic::catch_unwind(|| run_php("<?php $x = 1; echo isset($x + 1) ? 'yes' : 'no';"));
    assert!(result.is_err());
}

#[test]
fn test_isset_accepts_property_chains() {
    assert_eq!(
        run_php(
            "<?php
            class Boxed { public $value; }
            $null = null;
            $object = new Boxed();
            $object->value = new Boxed();
            $object->value->value = 42;
            var_dump(isset($null->missing->nested));
            var_dump(isset($object->value->value));
            var_dump(isset($object->value->missing));
            var_dump(isset($null->missing['key']->nested));
            "
        ),
        "bool(false)\nbool(true)\nbool(false)\nbool(false)\n"
    );
}

#[test]
fn test_multi_isset_short_circuits_property_magic() {
    assert_eq!(
        run_php(
            "<?php
            class MagicProbe {
                public function __isset($name) {
                    echo 'unexpected';
                    return true;
                }
            }
            $missing = null;
            $probe = new MagicProbe();
            var_dump(isset($missing, $probe->virtual));
            "
        ),
        "bool(false)\n"
    );
}

#[test]
fn test_chained_isset_calls_magic_in_php_order() {
    assert_eq!(
        run_php(
            "<?php
            class MagicChain {
                public function __isset($name) {
                    echo 'isset:' . $name . \"\\n\";
                    return $name !== 'missing';
                }
                public function __get($name) {
                    echo 'get:' . $name . \"\\n\";
                    return new MagicChain();
                }
            }
            $object = new MagicChain();
            var_dump(isset($object->first->second));
            var_dump(isset($object->missing->second));
            "
        ),
        "isset:first\nget:first\nisset:second\nbool(true)\nisset:missing\nbool(false)\n"
    );
}

#[test]
fn test_isset_object_property_uses_magic_only_when_unresolved() {
    assert_eq!(
        run_php(
            "<?php
            class MagicBox {
                public $present = null;
                public function __isset($name) {
                    echo 'magic:' . $name . \"\\n\";
                    return $name === 'virtual';
                }
            }
            $object = new MagicBox();
            var_dump(isset($object->present));
            var_dump(isset($object->virtual));
            var_dump(isset($object->missing));
            "
        ),
        "bool(false)\nmagic:virtual\nbool(true)\nmagic:missing\nbool(false)\n"
    );
}

#[test]
fn test_isset_inaccessible_property_does_not_leak_and_magic_exceptions_propagate() {
    assert_eq!(
        run_php(
            "<?php
            class HiddenBox {
                private $hiddenValue = 42;
                public function __isset($name) {
                    if ($name === 'boom') { throw new Exception('isset failed'); }
                    echo 'magic:' . $name . \"\\n\";
                    return false;
                }
                public function __get($name) { echo 'leaked'; return $this->hiddenValue; }
            }
            $object = new HiddenBox();
            var_dump(isset($object->hiddenValue->nested));
            try { isset($object->boom); } catch (Exception $error) {
                echo $error->getMessage() . \"\\n\";
            }
            "
        ),
        "magic:hiddenValue\nbool(false)\nisset failed\n"
    );
}

// ========== empty() ==========

#[test]
fn test_empty_undefined() {
    assert_eq!(run_php("<?php echo empty($x) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_empty_null() {
    assert_eq!(
        run_php("<?php $x = null; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_false() {
    assert_eq!(
        run_php("<?php $x = false; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_zero() {
    assert_eq!(
        run_php("<?php $x = 0; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_empty_string() {
    assert_eq!(
        run_php("<?php $x = ''; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_nonempty() {
    assert_eq!(
        run_php("<?php $x = 'hello'; echo empty($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_empty_array_empty() {
    assert_eq!(
        run_php("<?php $x = []; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_array_nonempty() {
    assert_eq!(
        run_php("<?php $x = [1]; echo empty($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn empty_silently_probes_uninitialized_typed_property_dimensions() {
    assert_eq!(
        run_php(
            r#"<?php
class EmptyTypedPropertyProbe {
    private array $values;
    public function missing(string $key): bool {
        return empty($this->values[$key]);
    }
    public function initialize(): void {
        $this->values = ['zero' => 0, 'value' => 4];
    }
}
class TypedArrayDimensionInitializer {
    private array $values;
    public function set(string $key, int $value): void {
        $this->values[$key] = $value;
    }
    public function get(string $key): int {
        return $this->values[$key];
    }
}
$probe = new EmptyTypedPropertyProbe();
echo $probe->missing('unset') ? 'silent:' : 'bad:';
$probe->initialize();
echo $probe->missing('zero') ? 'empty:' : 'bad:';
echo $probe->missing('value') ? 'bad:' : 'value:';
$initializer = new TypedArrayDimensionInitializer();
$initializer->set('created', 9);
echo $initializer->get('created');
"#,
        ),
        "silent:empty:value:9"
    );
}

// ========== unset() ==========

#[test]
fn test_unset_basic() {
    assert_eq!(
        run_php("<?php $x = 42; unset($x); echo isset($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_unset_multiple() {
    assert_eq!(
        run_php("<?php $a = 1; $b = 2; unset($a, $b); echo isset($a) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_unset_then_reassign() {
    assert_eq!(run_php("<?php $x = 1; unset($x); $x = 2; echo $x;"), "2");
}

#[test]
fn unset_detaches_one_local_name_without_mutating_shared_reference_value() {
    assert_eq!(
        run_php(
            "<?php $value = 'kept'; $first =& $value; $second =& $value; unset($first); echo $value, ':', $second, ':', isset($first) ? 'set' : 'gone', '|'; $first = 'fresh'; echo $value, ':', $second, ':', $first;"
        ),
        "kept:kept:gone|kept:kept:fresh"
    );
}

#[test]
fn unset_local_alias_preserves_array_and_property_reference_owners() {
    assert_eq!(
        run_php(
            "<?php $array = ['score' => 7]; $arrayAlias =& $array['score']; unset($arrayAlias); $array['score'] = 9; $object = (object) ['score' => 4]; $propertyAlias =& $object->score; unset($propertyAlias); $object->score = 6; echo $array['score'], ':', isset($arrayAlias) ? 'set' : 'gone', '|', $object->score, ':', isset($propertyAlias) ? 'set' : 'gone';"
        ),
        "9:gone|6:gone"
    );
}

#[test]
fn unset_last_local_alias_leaves_the_container_reference_value_materialized() {
    assert_eq!(
        run_php(
            "<?php $score = 12; $container = ['score' => &$score]; unset($score); $copy = [...$container]; echo $copy['score'], ':', $container['score'];"
        ),
        "12:12"
    );
}

#[test]
fn test_unset_array_element() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; unset($a[1]); echo count($a);"),
        "2"
    );
}

#[test]
fn test_unset_array_element_isset() {
    assert_eq!(
        run_php(
            "<?php $a = ['x' => 1, 'y' => 2]; unset($a['x']); echo isset($a['x']) ? 'yes' : 'no';"
        ),
        "no"
    );
}

#[test]
fn test_unset_array_preserves_other() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; unset($a[0]); echo $a[1] . $a[2];"),
        "23"
    );
}

#[test]
fn test_unset_dynamic_object_property() {
    assert_eq!(
        run_php(
            "<?php $state = (object)['route' => 'home', 'keep' => 1]; unset($state->route); echo (isset($state->route) ? 'set' : 'unset') . ':' . $state->keep;"
        ),
        "unset:1"
    );
}

#[test]
fn test_unset_declared_object_property() {
    assert_eq!(
        run_php(
            "<?php class Box { public $value = 1; } $box = new Box(); unset($box->value); echo isset($box->value) ? 'set' : 'unset';"
        ),
        "unset"
    );
}

// ========== Type casting ==========

#[test]
fn test_cast_int_from_float() {
    assert_eq!(run_php("<?php echo (int)3.7;"), "3");
}

#[test]
fn test_cast_int_from_string() {
    assert_eq!(run_php("<?php echo (int)'42abc';"), "42");
}

#[test]
fn test_cast_int_from_bool_true() {
    assert_eq!(run_php("<?php echo (int)true;"), "1");
}

#[test]
fn test_cast_int_from_bool_false() {
    assert_eq!(run_php("<?php echo (int)false;"), "0");
}

#[test]
fn test_cast_int_from_null() {
    assert_eq!(run_php("<?php echo (int)null;"), "0");
}

#[test]
fn test_cast_float_from_int() {
    assert_eq!(run_php("<?php $x = (float)42; echo $x + 0.5;"), "42.5");
}

#[test]
fn test_cast_float_from_string() {
    assert_eq!(run_php("<?php echo (float)'3.14';"), "3.14");
}

#[test]
fn test_cast_string_from_int() {
    assert_eq!(run_php("<?php $s = (string)42; echo strlen($s);"), "2");
}

#[test]
fn test_cast_string_from_float() {
    assert_eq!(run_php("<?php $s = (string)3.14; echo $s;"), "3.14");
}

#[test]
fn test_cast_string_from_bool() {
    assert_eq!(run_php("<?php echo (string)true;"), "1");
}

#[test]
fn test_cast_bool_truthy() {
    assert_eq!(run_php("<?php echo (bool)42 ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_cast_bool_falsy() {
    assert_eq!(run_php("<?php echo (bool)0 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_cast_array_from_scalar() {
    assert_eq!(
        run_php("<?php $a = (array)42; echo count($a) . ':' . $a[0];"),
        "1:42"
    );
}

#[test]
fn test_cast_array_from_null() {
    assert_eq!(run_php("<?php $a = (array)null; echo count($a);"), "0");
}

#[test]
fn test_cast_array_from_array() {
    assert_eq!(
        run_php("<?php $a = [1,2]; $b = (array)$a; echo count($b);"),
        "2"
    );
}

#[test]
fn object_to_array_cast_projects_properties_instead_of_wrapping_the_object() {
    assert_eq!(
        run_php(
            "<?php class CastNode { public $payload = ['ok']; } $cast = (array) new CastNode(); echo count($cast), ':', $cast['payload'][0];"
        ),
        "1:ok"
    );
}

#[test]
fn object_to_array_cast_mangles_visibility_and_skips_uninitialized_slots() {
    assert_eq!(
        run_php(
            "<?php #[AllowDynamicProperties] class CastBox { public $pub = 1; protected $prot = 2; private $priv = 3; public int $unset; } $box = new CastBox(); $box->dyn = 4; $keys = ['pub', \"\0*\0prot\", \"\0CastBox\0priv\", 'dyn']; $index = 0; $ok = true; foreach ((array) $box as $key => $value) { $ok = $ok && $key === $keys[$index] && $value === $index + 1; ++$index; } echo $ok ? 'OK:' : 'BAD:', $index;"
        ),
        "OK:4"
    );
}

#[test]
fn test_cast_object_from_array_scalar_and_null() {
    assert_eq!(
        run_php(
            "<?php $a = (object)['name' => 'route', 0 => 'zero']; $s = (object)42; $n = (object)null; echo $a->name . ':' . $s->scalar . ':' . (isset($n->missing) ? 'set' : 'empty');"
        ),
        "route:42:empty"
    );
}

#[test]
fn test_cast_object_keeps_object_identity() {
    assert_eq!(
        run_php(
            "<?php $a = (object)['value' => 1]; $b = (object)$a; $b->value = 2; echo $a->value;"
        ),
        "2"
    );
}

#[test]
fn test_cast_integer_keyword() {
    assert_eq!(run_php("<?php echo (integer)3.7;"), "3");
}

#[test]
fn test_cast_double_keyword() {
    assert_eq!(run_php("<?php $x = (double)42; echo $x + 0.5;"), "42.5");
}

#[test]
fn test_cast_boolean_keyword() {
    assert_eq!(run_php("<?php echo (boolean)1 ? 'yes' : 'no';"), "yes");
}

// ========== Practical combined ==========

#[test]
fn test_practical_null_safe_default() {
    assert_eq!(
        run_php(
            "<?php
$config = null;
$value = isset($config) ? $config : 'default';
echo $value;
"
        ),
        "default"
    );
}

#[test]
fn test_practical_type_check_pattern() {
    assert_eq!(
        run_php(
            "<?php
$items = [1, 'two', 3, 'four', 5];
$sum = 0;
foreach ($items as $v) {
    if (is_int($v)) {
        $sum += $v;
    }
}
echo $sum;
"
        ),
        "9"
    );
}

#[test]
fn test_practical_isset_with_unset_loop() {
    assert_eq!(
        run_php(
            "<?php
$a = 1; $b = 2; $c = 3;
unset($b);
$result = '';
if (isset($a)) { $result .= 'a'; }
if (isset($b)) { $result .= 'b'; }
if (isset($c)) { $result .= 'c'; }
echo $result;
"
        ),
        "ac"
    );
}

#[test]
fn test_practical_cast_sum_strings() {
    assert_eq!(
        run_php(
            "<?php
$a = '10';
$b = '20';
echo (int)$a + (int)$b;
"
        ),
        "30"
    );
}

// ========== empty() with expressions ==========

#[test]
fn test_empty_expression_truthy() {
    assert_eq!(
        run_php("<?php $x = 1; echo empty($x + 1) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_empty_expression_falsy() {
    assert_eq!(
        run_php("<?php $x = 0; echo empty($x + 0) ? 'yes' : 'no';"),
        "yes"
    );
}

// ========== unset() on non-array ==========

#[test]
fn test_unset_dim_on_scalar_fatal() {
    let result = std::panic::catch_unwind(|| run_php("<?php $x = 42; unset($x[0]);"));
    assert!(result.is_err());
}

#[test]
fn test_unset_dim_on_null_silent() {
    assert_eq!(run_php("<?php $x = null; unset($x[0]); echo 'ok';"), "ok");
}

#[test]
fn test_unset_dim_on_undef_warns_and_continues() {
    assert_eq!(
        run_php("<?php unset($x[0]); echo 'ok';"),
        "\nWarning: Undefined variable $x in <main> on line 1\nok"
    );
}
