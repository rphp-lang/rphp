mod common;

use common::run_php;

#[test]
fn catch_retires_eval_snapshot_before_entering_handler() {
    assert_eq!(
        run_php(
            r#"<?php
class CatchOwner { function __destruct() { echo 'drop|'; } }
$slot = new CatchOwner; eval('1;');
try { throw new RuntimeException('new'); } catch (Throwable $slot) { echo 'catch|'; }
echo get_class($GLOBALS['slot']), '|';
"#
        ),
        "drop|catch|RuntimeException|"
    );
}

#[test]
fn finally_catch_retires_the_same_symbol_snapshot() {
    assert_eq!(
        run_php(
            r#"<?php
class FinallyCatchOwner { function __destruct() { echo 'drop|'; } }
$slot = new FinallyCatchOwner; eval('1;');
try { try { throw new RuntimeException('new'); }
catch (Throwable $slot) { echo 'catch|'; } finally { echo 'finally|'; } }
catch (Throwable $outer) { echo 'unexpected|'; }
echo 'end|';
"#
        ),
        "drop|catch|finally|end|"
    );
}

#[test]
fn catch_snapshot_preserves_real_owners_and_reference_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
class SharedCatchOwner { function __destruct() { echo 'drop|'; } }
$slot = new SharedCatchOwner; $saved = $slot; $alias =& $slot; eval('1;');
try { throw new RuntimeException('new'); } catch (Throwable $slot) {
    echo get_class($alias), ':', get_class($saved), '|';
}
unset($saved); echo 'end|';
"#
        ),
        "RuntimeException:SharedCatchOwner|drop|end|"
    );
}

#[test]
fn local_eval_catch_does_not_retire_shadowed_global() {
    assert_eq!(
        run_php(
            r#"<?php
class OuterCatchOwner { function __destruct() { echo 'outer-drop|'; } }
class InnerCatchOwner { function __destruct() { echo 'inner-drop|'; } }
$slot = new OuterCatchOwner;
function localCatch() {
    $slot = new InnerCatchOwner; eval('1;');
    try { throw new RuntimeException('new'); } catch (Throwable $slot) { echo 'catch|'; }
}
localCatch(); echo get_class($slot), '|'; unset($slot); echo 'end|';
"#
        ),
        "inner-drop|catch|OuterCatchOwner|outer-drop|end|"
    );
}

#[test]
fn failed_constructor_operands_retire_before_the_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
class UnfinishedEnvelope { function __construct($first, $second) {} }
$anchor = new stdClass;
try { static $broken = new UnfinishedEnvelope(new stdClass, [] + 1); }
catch (Throwable $error) { echo get_class($error), '|'; }
$next = new stdClass; $following = new stdClass;
echo spl_object_id($anchor), ':', spl_object_id($next), ':', spl_object_id($following), '|';
"#
        ),
        "TypeError|1:2:3|"
    );
}

#[test]
fn eval_failed_constructor_operands_keep_the_same_dependency_order() {
    assert_eq!(
        run_php(
            r#"<?php
class EvalUnfinishedEnvelope { function __construct($first, $second) {} }
$anchor = new stdClass;
try { eval('new EvalUnfinishedEnvelope(new stdClass, [] + 1);'); }
catch (Throwable $error) { echo get_class($error), '|'; }
$next = new stdClass; $following = new stdClass;
echo spl_object_id($anchor), ':', spl_object_id($next), ':', spl_object_id($following), '|';
"#
        ),
        "TypeError|1:2:3|"
    );
}

#[test]
fn failed_ordinary_call_arguments_keep_left_to_right_release() {
    assert_eq!(
        run_php(
            r#"<?php
class OrderedCallArgument { function __construct(public $name) {} function __destruct() { echo $this->name, '|'; } }
function unusedCall($first, $second, $third) { echo 'unexpected|'; }
$anchor = new stdClass;
try { unusedCall(new OrderedCallArgument('one'), new OrderedCallArgument('two'), [] + 1); }
catch (Throwable $error) { echo get_class($error), '|'; }
$next = new stdClass; $following = new stdClass;
echo spl_object_id($anchor), ':', spl_object_id($next), ':', spl_object_id($following), '|';
"#
        ),
        "one|two|TypeError|1:3:2|"
    );
}

#[test]
fn exception_trace_ownership_respects_both_argument_settings() {
    assert_eq!(
        common::run_php_with_source_context(
            r#"<?php
class TraceArgument { function __destruct() { echo 'drop|'; } }
function traceFailure($object) { throw new RuntimeException('stop'); }
foreach ([0, 1] as $ignore) {
    ini_set('zend.exception_ignore_args', (string) $ignore); echo $ignore, '|';
    try { static $stored = traceFailure(new TraceArgument); }
    catch (Throwable $error) { echo 'catch|'; }
    unset($error); echo 'end|';
}
"#,
            "failed-expression.php",
            ".",
        ),
        "0|catch|drop|end|1|drop|catch|end|"
    );
}

#[test]
fn eval_failed_expression_preserves_shared_local_owners() {
    assert_eq!(
        run_php(
            r#"<?php
class SharedLocal { function __destruct() { echo 'local-drop|'; } }
class EvalTemporary { function __destruct() { echo 'temporary-drop|'; } }
class EvalPending { function __construct($first, $second) {} }
function localEvalScope() {
    $saved = new SharedLocal;
    try { eval('$copy = $saved; new EvalPending(new EvalTemporary, [] + 1);'); }
    catch (Throwable $error) { echo 'catch|'; }
    echo (int) ($saved === $copy), '|'; unset($copy, $saved); echo 'after|';
}
localEvalScope();
"#
        ),
        "temporary-drop|catch|1|local-drop|after|"
    );
}

#[test]
fn eval_cleanup_chains_multiple_destructor_exceptions() {
    assert_eq!(
        run_php(
            r#"<?php
class ChainedTemporary { function __construct(public $name) {} function __destruct() {
    echo $this->name, '|'; throw new LogicException($this->name); } }
class ChainedOwner { function __construct($first, $second, $third) {} }
try { eval('new ChainedOwner(new ChainedTemporary("one"), new ChainedTemporary("two"), [] + 1);'); }
catch (Throwable $error) { do { echo get_class($error), ':', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious()); }
echo 'after|';
"#
        ),
        "one|two|LogicException:two|LogicException:one|TypeError:Unsupported operand types: array + int|after|"
    );
}

#[test]
fn failed_include_retires_temporaries_before_caller_catch() {
    assert_eq!(
        run_php(
            r#"<?php
class IncludedTemporary { function __destruct() { echo 'drop|'; } }
class IncludedOwner { function __construct($first, $second) {} }
$path = tempnam(sys_get_temp_dir(), 'expression-');
file_put_contents($path, '<?php new IncludedOwner(new IncludedTemporary, [] + 1);');
try { include $path; } catch (Throwable $error) { echo get_class($error), '|'; }
finally { unlink($path); }
echo 'after|';
"#
        ),
        "drop|TypeError|after|"
    );
}

#[test]
fn eval_cleanup_resurrection_keeps_the_escaping_owner() {
    assert_eq!(
        run_php(
            r#"<?php
class EscapingTemporary { public $value = 19; function __destruct() {
    echo 'drop|'; $GLOBALS['escaped'] = $this; } }
class EscapingOwner { function __construct($first, $second) {} }
try { eval('new EscapingOwner(new EscapingTemporary, [] + 1);'); }
catch (Throwable $error) { echo get_class($error), '|'; }
echo $escaped->value, '|'; unset($escaped); echo 'after|';
"#
        ),
        "drop|TypeError|19|after|"
    );
}

#[test]
fn static_array_argument_cleanup_retires_only_uncommitted_owners() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrayTemporary { function __destruct() { echo 'drop|'; } }
function staticArrayValue($items) { echo 'call|'; return 23; }
function initializeArrayArgument() { static $value = staticArrayValue([new ArrayTemporary]); echo $value, '|'; }
initializeArrayArgument(); initializeArrayArgument();
"#
        ),
        "call|drop|23|23|"
    );
}

#[test]
fn static_nested_argument_retirement() {
    assert_eq!(
        run_php(
            r#"<?php
class RetiredArgument { function __destruct() { echo 'drop|'; } }
class PendingOwner { function __construct($first, $second) { echo 'unexpected|'; } }
try { static $slot = new PendingOwner(new RetiredArgument, [] + 1); }
catch (Throwable $error) { echo get_class($error), '|'; }
echo 'after|';
"#
        ),
        "drop|TypeError|after|"
    );
}

#[test]
fn static_cleanup_precedes_finally() {
    assert_eq!(
        run_php(
            r#"<?php
class FinallyArgument { function __destruct() { echo 'drop|'; } }
class FinallyOwner { function __construct($first, $second) {} }
try { try { static $slot = new FinallyOwner(new FinallyArgument, [] + 1); }
finally { echo 'finally|'; } }
catch (Throwable $error) { echo get_class($error), '|'; }
echo 'after|';
"#
        ),
        "drop|finally|TypeError|after|"
    );
}

#[test]
fn static_retry_and_committed_value() {
    assert_eq!(
        run_php(
            r#"<?php
class RetryArgument { function __destruct() { echo 'drop|'; } }
function nextAttempt() { static $attempt = 0; echo 'attempt', ++$attempt, '|';
    if ($attempt < 3) throw new RuntimeException('retry'); return 17; }
function makeValue($object, $number) { echo 'make|'; return $number; }
function retriedValue() { try { static $value = makeValue(new RetryArgument, nextAttempt());
    echo 'value', $value, '|'; } catch (Throwable $error) { echo 'catch|'; } }
retriedValue(); retriedValue(); retriedValue(); retriedValue();
"#
        ),
        "attempt1|drop|catch|attempt2|drop|catch|attempt3|make|drop|value17|value17|"
    );
}

#[test]
fn static_multiple_declarations_preserve_completed_prefix() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class PrefixArgument { function __destruct() { echo 'drop|'; } }
function prefixValue() { echo 'prefix|'; return 11; }
function prefixFail($object) { throw new RuntimeException('stop'); }
function prefixDeclarations() { try { static $first = prefixValue(), $second = prefixFail(new PrefixArgument); }
    catch (Throwable $error) { echo $first, ':catch|'; } }
prefixDeclarations(); prefixDeclarations();
"#
        ),
        "prefix|drop|11:catch|drop|11:catch|"
    );
}

#[test]
fn static_cleanup_exception_replaces_original() {
    assert_eq!(
        run_php(
            r#"<?php
class ThrowingArgument { function __destruct() { echo 'drop|'; throw new LogicException('cleanup'); } }
class ThrowingOwner { function __construct($first, $second) {} }
try { static $slot = new ThrowingOwner(new ThrowingArgument, [] + 1); }
catch (Throwable $error) { do { echo get_class($error), ':', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious()); }
echo 'after|';
"#
        ),
        "drop|LogicException:cleanup|TypeError:Unsupported operand types: array + int|after|"
    );
}

#[test]
fn recursive_static_initialization_keeps_committed_cell() {
    assert_eq!(
        run_php(
            r#"<?php
function recursiveValue($depth) {
    static $value = $depth ? recursiveValue(0) + 1 : 7;
    echo $depth, ':', $value, '|'; return $value;
}
recursiveValue(1); recursiveValue(2);
"#
        ),
        "0:7|1:7|2:7|"
    );
}

#[test]
fn eval_static_cleanup_precedes_caller_handler() {
    assert_eq!(
        run_php(
            r#"<?php
class EvalArgument { function __destruct() { echo 'drop|'; } }
class EvalOwner { function __construct($first, $second) {} }
try { eval('static $slot = new EvalOwner(new EvalArgument, [] + 1);'); }
catch (Throwable $error) { echo get_class($error), '|'; }
echo 'after|';
"#
        ),
        "drop|TypeError|after|"
    );
}

#[test]
fn ordinary_and_default_construction_order_stays_exact() {
    assert_eq!(
        run_php(
            r#"<?php
class ConstructionItem { function __construct() { echo 'item|'; } }
class ConstructionOwner { function __construct(public $item) { echo 'owner|'; } }
function withDefault($value = new ConstructionOwner(new ConstructionItem)) { echo 'body|'; }
$ordinary = new ConstructionOwner(new ConstructionItem);
static $stored = new ConstructionOwner(new ConstructionItem);
withDefault(); withDefault(); echo 'done|';
"#
        ),
        "item|owner|item|owner|item|owner|body|item|owner|body|done|"
    );
}

#[test]
fn nested_string_cast_write_unset_and_compound() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($level, $message) { echo $message, '|'; });
foreach ([null, false, true, 1.5, INF] as $key) {
    foreach (['write', 'unset', 'compound'] as $operation) {
        $word = 'abcd'; try {
            if ($operation === 'write') $word[$key][0] = 'z';
            elseif ($operation === 'unset') unset($word[$key][0]);
            else $word[$key][0] += 'z';
        } catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
        echo $word, '|';
    }
}
"#
        ),
        "String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|String offset cast occurred|Error:Cannot use string offset as an array|abcd|"
    );
}

#[test]
fn silent_string_probes_and_numeric_string_priority() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($level, $message) { echo $message, '|'; });
foreach ([null, false, true] as $key) {
    $word = 'abcd'; echo (int) isset($word[$key]), ':', (int) empty($word[$key]), ':', $word[$key] ?? 'missing', '|';
}
foreach (['1tail', 'bad', [], new stdClass] as $key) {
    $word = 'abcd'; try { unset($word[$key][0]); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
    echo $word, '|';
}
"#
        ),
        "1:0:a|1:0:a|1:0:b|Error:Cannot use string offset as an array|abcd|Error:Cannot unset string offsets|abcd|Error:Cannot unset string offsets|abcd|Error:Cannot unset string offsets|abcd|"
    );
}

#[test]
fn diagnostic_exception_precedes_nested_write_error() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($level, $message) { echo 'warning|'; throw new LogicException('handler'); });
$word = 'abcd'; try { $word[null][0] = 'z'; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
echo $word, '|';
"#
        ),
        "warning|LogicException:handler|abcd|"
    );
}

#[test]
fn diagnostic_receiver_replacement_preserves_new_state() {
    assert_eq!(
        run_php(
            r#"<?php
$word = 'abcd'; set_error_handler(function($level, $message) use (&$word) {
    echo 'warning|'; $word = ['kept'];
});
try { $word[null][0] = 'z'; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
echo json_encode($word), '|';
$word = 'abcd'; try { unset($word[false][0]); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
echo json_encode($word), '|';
"#
        ),
        "warning|Error:Cannot use string offset as an array|[\"kept\"]|warning|Error:Cannot use string offset as an array|[\"kept\"]|"
    );
}

#[test]
fn diagnostic_key_replacement_does_not_repeat_evaluation() {
    assert_eq!(
        run_php(
            r#"<?php
$key = null; $calls = 0; function readOffset() { global $key, $calls; ++$calls; return $key; }
set_error_handler(function($level, $message) use (&$key) { echo 'warning|'; $key = []; });
$word = 'abcd'; try { $word[readOffset()][0] = 'z'; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
echo $calls, ':', json_encode($key), ':', $word, '|';
"#
        ),
        "warning|Error:Cannot use string offset as an array|1:[]:abcd|"
    );
}

#[test]
fn nested_string_reference_and_object_contexts() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($level, $message) { echo $message, '|'; });
$word = 'abcd'; try { $reference =& $word[null][0]; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
try { $word[false]->value = 1; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
try { ++$word[true][0]; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
echo $word, '|';
"#
        ),
        "String offset cast occurred|Error:Cannot use string offset as an array|String offset cast occurred|Error:Cannot use string offset as an object|String offset cast occurred|Error:Cannot use string offset as an array|abcd|"
    );
}
