mod common;

use common::run_php;

#[test]
fn casting_a_container_retires_only_its_unshared_children() {
    assert_eq!(
        run_php(
            r#"<?php
class ProjectedCompletionOwner {
    public function __construct(public $name) {}
    public function __destruct() { echo $this->name, '|'; }
}
$owner = new ProjectedCompletionOwner('shared');
$alias =& $owner;
$record = (object) ['slot' => &$owner];
echo 'cast|';
$record->slot = null;
echo 'cleared|';
unset($owner, $alias, $record);
$nested = [new ProjectedCompletionOwner('nested')];
$record = (object) ['slot' => $nested];
unset($record);
echo 'kept|';
unset($nested);
$record = (object) ['slot' => new ProjectedCompletionOwner('private')];
echo 'private-cast|';
unset($record);
function privateCompletionAliases() {
    $value = new ProjectedCompletionOwner('aliases');
    return [&$value, &$value];
}
echo (int) privateCompletionAliases(), '|';
echo 'done';
"#
        ),
        "cast|shared|cleared|kept|nested|private-cast|private|aliases|1|done"
    );
}

#[test]
fn replaced_dimension_values_retire_after_commit_before_result_consumption() {
    assert_eq!(
        run_php(
            r#"<?php
class RetiredCompletionElement {
    public function __construct(public $name) {}
    public function __destruct() { echo $this->name, '|'; throw new Exception('retired'); }
}
function consumeDimensionCompletion($value) { echo 'body|'; }
$items = [new RetiredCompletionElement('direct')];
try { consumeDimensionCompletion($items[0] = 21); }
catch (Exception $error) { echo $items[0], '|'; }
$items = [[new RetiredCompletionElement('nested')]];
try { consumeDimensionCompletion($items[0] = 22); }
catch (Exception $error) { echo $items[0], '|'; }
$items = [new RetiredCompletionElement('shared')];
$copy = $items;
$items[0] = 23;
echo 'copy-held|';
try { unset($copy); } catch (Exception $error) { echo 'copy-gone|'; }
echo 'done';
"#
        ),
        "direct|21|nested|22|copy-held|shared|copy-gone|done"
    );
}

#[test]
fn reference_assignment_retains_its_source_when_the_old_destructor_clears_the_target() {
    assert_eq!(
        run_php(
            r#"<?php
class ClearedCompletionDimension {
    public function __destruct() { $GLOBALS['completionItems'] = null; echo 'drop|'; }
}
function consumeCompletedReference($value) { echo $value->number, '|'; }
$completionItems = [new ClearedCompletionDimension];
$source = (object) ['number' => 17];
consumeCompletedReference($completionItems[0] =& $source);
echo $source->number, ':', gettype($completionItems), '|';
$completionItems = [];
$values = ['number' => 19];
$completionItems[0] =& $values['number'];
$values['number'] = 23;
echo $completionItems[0], '|';
$completionItems[0] = 29;
echo $values['number'];
"#
        ),
        "drop|17|17:NULL|23|29"
    );
}

#[test]
fn native_dimension_binding_keeps_existing_source_aliases_but_method_writes_are_values() {
    assert_eq!(
        run_php(
            r#"<?php
$map = new WeakMap;
$key = new stdClass;
$other = new stdClass;
$number = 4;
$alias =& $number;
$map[$key] = 0;
$map[$key] =& $number;
$number = 7;
echo $map[$key], '|';
$map[$key] = 9;
echo $number, ':', $alias, '|';
$map->offsetSet($other, $number);
$number = 11;
echo $map[$other], '|', $alias;
"#
        ),
        "7|7:7|7|11"
    );
}

#[test]
fn replacing_a_native_dimension_reference_keeps_the_old_external_owner_alive() {
    assert_eq!(
        run_php(
            r#"<?php
class NativeCompletionOwner {
    public function __destruct() { echo 'drop|'; }
}
$map = new WeakMap;
$key = new stdClass;
$owner = new NativeCompletionOwner;
$alias =& $owner;
$map[$key] = null;
$map[$key] =& $owner;
$map[$key] = 18;
echo 'held:', is_object($alias) ? 1 : 0, '|';
unset($owner, $alias);
echo $map[$key];
"#
        ),
        "held:1|drop|18"
    );
}

#[test]
fn magic_reference_reads_are_values_for_direct_read_modify_write() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo $message, '|'; });
class CompletionReadCell {
    public $stored = 10;
    public function &__get($name) { echo 'get|'; return $this->stored; }
}
$object = new CompletionReadCell;
$result = ($object->extra += 2);
echo $object->stored, ':', $object->extra, ':', $result, '|';
$object = new CompletionReadCell;
$result = $object->extra++;
echo $object->stored, ':', $object->extra, ':', $result, '|';
$object = new CompletionReadCell;
$result = ++$object->extra;
echo $object->stored, ':', $object->extra, ':', $result, '|';
$object = new CompletionReadCell;
$alias =& $object->extra;
$alias = 30;
echo $object->stored, ':', count(get_object_vars($object));
"#
        ),
        "get|Creation of dynamic property CompletionReadCell::$extra is deprecated|10:12:12|get|Creation of dynamic property CompletionReadCell::$extra is deprecated|10:11:10|get|Creation of dynamic property CompletionReadCell::$extra is deprecated|10:11:11|get|30:1"
    );
}

#[test]
fn captured_borrowed_arguments_become_owned_before_operand_retirement() {
    assert_eq!(
        run_php(
            r#"<?php
function completeCapturedArgument($text, $parts) {
    preg_replace_callback('/x/', function ($match) use (&$parts, &$text) {
        $text = preg_replace('/x/', array_shift($parts), $text, 1);
    }, $text);
    return $text;
}
$shared = ['a', 'b'];
echo completeCapturedArgument('x', ['a']) . '|';
echo completeCapturedArgument('xx', $shared) . '|';
echo implode(',', $shared), '|';
echo completeCapturedArgument('xxx', ['c', 'd', 'e']) . '|done';
"#
        ),
        "a|ab|a,b|cde|done"
    );
}

#[test]
fn temporary_receivers_retire_before_their_read_or_update_result_is_consumed() {
    assert_eq!(
        run_php(
            r#"<?php
class TemporaryCompletionReceiver {
    public $number = 40;
    public $child;
    public function __construct() { $this->child = (object) ['value' => 9]; }
    public function __destruct() { echo 'drop|'; }
    public function __toString() { return 'text'; }
}
function completedRead($value) { echo is_object($value) ? $value->value : $value, '|'; }
function makeTemporaryCompletionReceiver() { return new TemporaryCompletionReceiver; }
completedRead((new TemporaryCompletionReceiver)->child);
completedRead(makeTemporaryCompletionReceiver()->number++);
completedRead(++makeTemporaryCompletionReceiver()->number);
$name = 'child';
completedRead((new TemporaryCompletionReceiver)->{$name});
completedRead((string) new TemporaryCompletionReceiver);
completedRead(isset((new TemporaryCompletionReceiver)->child->value));
echo 'done';
"#
        ),
        "drop|9|drop|40|drop|41|drop|9|drop|text|drop|1|done"
    );
}

#[test]
fn failed_dimension_read_retires_pending_operand_trees_before_catching() {
    assert_eq!(
        run_php(
            r#"<?php
class InvalidCompletionOffset implements ArrayAccess {
    public $items = [1];
    public function &offsetGet($key): bool { return $this->items; }
    public function offsetSet($key, $value): void {}
    public function offsetExists($key): bool { return true; }
    public function offsetUnset($key): void {}
}
class FailedCompletionOperand {
    public function __destruct() { echo 'drop|'; throw new RuntimeException('cleanup'); }
}
function consumeFailedCompletion($value) { echo 'body|'; }
$object = new InvalidCompletionOffset;
try { consumeFailedCompletion($object[0] += [new FailedCompletionOperand]); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
echo 'alive';
"#
        ),
        "drop|RuntimeException:cleanup|alive"
    );
}

#[test]
fn destructor_rethrow_does_not_replace_an_empty_creation_trace() {
    let source = r#"<?php
class SavedCompletionFailure {
    public function __construct(public $failure) {}
    public function __destruct() {
        if ($this->failure) { throw $this->failure; }
        throw new RuntimeException('fresh');
    }
}
$failure = new RuntimeException('old');
foreach ([$failure, null] as $value) {
    try { $owner = new SavedCompletionFailure($value); unset($owner); }
    catch (RuntimeException $caught) {
        echo $caught->getMessage(), ':', count($caught->getTrace()), '|';
    }
}
echo count($failure->getTrace());
"#;
    assert_eq!(
        common::run_php_with_source_context(source, "completion.php", "."),
        "old:0|fresh:1|0"
    );
}

#[test]
fn reference_return_error_keeps_the_selected_return_origin() {
    let source = r#"<?php
function &locatedCompletion($branch): int {
    $cell = 2;
    try {
        if ($branch) { return $cell; }
        return $cell;
    } finally {
        $cell = 'invalid';
    }
}
foreach ([true, false] as $branch) {
    try { locatedCompletion($branch); }
    catch (TypeError $error) {
        echo $error->getLine(), ':', basename($error->getFile()), ':';
        echo $error->getTrace()[0]['function'], '|';
    }
}
"#;
    assert_eq!(
        common::run_php_with_source_context(source, "completion.php", "."),
        "5:completion.php:locatedCompletion|6:completion.php:locatedCompletion|"
    );
}

#[test]
fn delayed_return_origin_is_not_part_of_the_php_variable_scope() {
    assert_eq!(
        run_php(
            r#"<?php
function &hiddenCompletion(): int {
    $cell = 1;
    try { return $cell; }
    finally {
        echo implode(',', array_keys(get_defined_vars())), '|';
    }
}
hiddenCompletion();
echo hiddenCompletion();
"#
        ),
        "cell|cell|1"
    );
}

#[test]
fn compound_operand_cleanup_happens_after_writeback_before_consumption() {
    assert_eq!(
        run_php(
            r#"<?php
class RetiredUnionValue {
    public function __destruct() { echo 'drop|'; throw new Exception('retired'); }
}
class CompletedUnionBox { public $items = [13]; }
function consumeCompletedUnion($items) { echo 'consume|'; }
$items = [12];
try { consumeCompletedUnion($items += [new RetiredUnionValue]); }
catch (Exception $error) { echo 'cv:', $items[0], '|'; }
$box = new CompletedUnionBox;
try { consumeCompletedUnion($box->items += [new RetiredUnionValue]); }
catch (Exception $error) { echo 'property:', $box->items[0], '|'; }
echo 'done';
"#
        ),
        "drop|cv:12|drop|property:13|done"
    );
}

#[test]
fn virtual_property_projections_separate_readable_values_from_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class ProjectionCompletion {
    public $stored = 2 { get => $this->stored + 1; }
    public $computed { get => 5; }
    public $sink { set {} }
    private $hidden = 7 { get => $this->hidden + 1; }
    public function visible() { return get_object_vars($this); }
}
$object = new ProjectionCompletion;
echo json_encode(get_object_vars($object)), '|';
echo count((array) $object), ':', count(get_mangled_object_vars($object)), '|';
echo json_encode($object->visible()), '|';
$export = var_export($object, true);
echo strpos($export, "'sink'") === false ? 'no-sink' : 'sink', '|';
echo strpos($export, "'hidden' => 8") !== false ? 'getter' : 'raw';
"#
        ),
        "{\"stored\":3,\"computed\":5}|2:2|{\"stored\":3,\"computed\":5,\"hidden\":8}|no-sink|getter"
    );
}

#[test]
fn unused_reference_returns_validate_the_live_cell_after_nested_finally() {
    assert_eq!(
        run_php(
            r#"<?php
function &discardedCompletion(): int {
    $cell = 7;
    try { return $cell; }
    finally {
        try { return $cell; }
        finally { $cell = 'invalid'; }
    }
}
try { discardedCompletion(); }
catch (TypeError $error) { echo $error->getMessage(), '|'; }
$callback = 'discardedCompletion';
try { $callback(); }
catch (TypeError $error) { echo $error->getMessage(), '|'; }
echo 'alive';
"#
        ),
        "discardedCompletion(): Return value must be of type int, string returned|discardedCompletion(): Return value must be of type int, string returned|alive"
    );
}

#[test]
fn unused_method_and_closure_returns_keep_their_finally_contract() {
    assert_eq!(
        run_php(
            r#"<?php
class CompletionMethods {
    public function &finish(): int {
        $value = 3;
        try { return $value; }
        finally { $value = []; }
    }
}
$object = new CompletionMethods;
try { $object->finish(); }
catch (TypeError $error) { echo 'method:', $error->getMessage(), '|'; }
$callback = function &(): int {
    $value = 9;
    try { return $value; }
    finally { $value = []; }
};
try { $callback(); }
catch (TypeError $error) { echo 'closure:', get_class($error), '|'; }
echo 'done';
"#
        ),
        "method:CompletionMethods::finish(): Return value must be of type int, array returned|closure:TypeError|done"
    );
}

#[test]
fn unused_reference_return_coercion_updates_the_original_cell_only() {
    assert_eq!(
        run_php(
            r#"<?php
function &coercedCompletion(&$cell): int {
    try { return $cell; }
    finally { $cell = '23'; }
}
$cell = 4;
coercedCompletion($cell);
echo gettype($cell), ':', $cell, '|';
function copiedCompletion(&$cell): int {
    try { return $cell; }
    finally { $cell = 'different'; }
}
$cell = 5;
echo copiedCompletion($cell), ':', $cell, '|';
function &replacedCompletion(): int {
    $bad = 1;
    $good = 17;
    try { return $bad; }
    finally { $bad = []; return $good; }
}
replacedCompletion();
echo replacedCompletion();
"#
        ),
        "integer:23|5:different|17"
    );
}

#[test]
fn concat_operand_destructor_throws_before_result_consumption() {
    assert_eq!(
        run_php(
            r#"<?php
class ConcatCompletion {
    public function __toString() { echo 'string|'; return 'value'; }
    public function __destruct() { echo 'drop|'; throw new Exception('retire'); }
}
function consumeConcat($value) { echo 'body:', $value, '|'; }
try { consumeConcat((new ConcatCompletion) . ':tail'); }
catch (Exception $error) { echo 'caught:', $error->getMessage(), '|'; }
echo 'done';
"#
        ),
        "string|drop|caught:retire|done"
    );
}

#[test]
fn nested_operand_cleanup_preserves_the_pending_call_and_shared_owners() {
    assert_eq!(
        run_php(
            r#"<?php
class UnionCompletion {
    public function __construct(public $name) {}
    public function __destruct() { echo 'drop:', $this->name, '|'; }
}
function nextUnionArgument() { echo 'next|'; return 'last'; }
function consumeUnion($first, $values, $last) {
    echo $first, ':', $values[0], ':', $last, '|';
}
consumeUnion('first', [11] + [new UnionCompletion('temporary')], nextUnionArgument());
$shared = [new UnionCompletion('shared')];
consumeUnion('second', [12] + $shared, nextUnionArgument());
echo 'held|';
unset($shared);
echo 'done';
"#
        ),
        "drop:temporary|next|first:11:last|next|second:12:last|held|drop:shared|done"
    );
}
