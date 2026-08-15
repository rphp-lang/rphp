/// E2E tests: isset, empty, unset, type casting, type checks.
mod common;
use common::{run_php, run_php_with_source_context};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::opcode::OpCode;

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
fn reflection_class_lazy_ghost_eagerly_runs_initializer_on_real_instance() {
    assert_eq!(
        run_php(
            "<?php class LazyService { public string $value; public function __construct(string $value) { $this->value = $value; } } $service = (new ReflectionClass(LazyService::class))->newLazyGhost(static function ($ghost) { $ghost->__construct('OK'); }); echo $service->value;"
        ),
        "OK"
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
            "<?php class CastBox { public $pub = 1; protected $prot = 2; private $priv = 3; public int $unset; } $box = new CastBox(); $box->dyn = 4; $keys = ['pub', \"\0*\0prot\", \"\0CastBox\0priv\", 'dyn']; $index = 0; $ok = true; foreach ((array) $box as $key => $value) { $ok = $ok && $key === $keys[$index] && $value === $index + 1; ++$index; } echo $ok ? 'OK:' : 'BAD:', $index;"
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
fn test_unset_dim_on_undef_silent() {
    assert_eq!(run_php("<?php unset($x[0]); echo 'ok';"), "ok");
}
