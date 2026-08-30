/// E2E tests: arrays — literals, access, assignment, push, nested, functions.
mod common;
use common::{run_php, run_php_expect_error};

include!("e2e_arrays/basic_operations.rs");

include!("e2e_arrays/copy_on_write.rs");

include!("e2e_arrays/mutation_and_hot_paths.rs");

include!("e2e_arrays/unpack_semantics.rs");

#[test]
fn overloaded_dimension_reference_arguments_do_not_write_back_detached_values() {
    let source = r#"<?php
function same_ref(&$left, &$right) { return $left === $right; }
class RefTree implements ArrayAccess {
    public array $values = [];
    public function offsetExists($key): bool { return isset($this->values[$key]); }
    public function offsetUnset($key): void { unset($this->values[$key]); }
    public function offsetSet($key, $value): void { echo "set|"; $this->values[$key] = $value; }
    public function offsetGet($key): mixed { echo "get|"; return $this->values[$key]; }
}
$tree = new RefTree; $value = new stdClass; $tree['key'] = $value;
var_dump(same_ref($tree['key'], $value));
"#;
    assert_eq!(run_php(source), "set|get|bool(true)\n");
}

#[test]
fn array_object_dimensions_keep_live_references_and_detach_value_copies() {
    let source = r#"<?php
$box = new ArrayObject(['k' => ['n' => 1]]);
$copy = $box['k'];
$alias =& $box['k'];
$alias['n'] += 2;
$box['k']['m'] = 4;
unset($box['k']['n']);
$copy['n'] = 9;
var_dump($box['k'], $alias, $copy, count($box));
"#;
    assert_eq!(
        run_php(source),
        "array(1) {\n  [\"m\"]=>\n  int(4)\n}\narray(1) {\n  [\"m\"]=>\n  int(4)\n}\narray(1) {\n  [\"n\"]=>\n  int(9)\n}\nint(1)\n"
    );
}

#[test]
fn array_object_anonymous_and_invalid_dimensions_preserve_receiver_state() {
    let source = r#"<?php
set_error_handler(function($level, $message) {
    echo "diag:", $message, "|";
    return true;
});
$exact = new ArrayObject;
$exact[][1] = 'value';
restore_error_handler();
error_reporting(0);
class ChildArrayObject extends ArrayObject {
    public function offsetGet($key): mixed { return 1; }
}
$child = new ChildArrayObject;
try { $child[][1] = 'value'; } catch (Throwable $e) {
    echo $e::class, ":", $e->getMessage(), "|";
}
$invalid = new ArrayObject;
try { $invalid[[]] += 1; } catch (Throwable $e) {
    echo $e::class, ":", $e->getMessage(), "|";
}
try { unset($invalid[[]][[]]); } catch (Throwable $e) {
    echo $e::class, ":", $e->getMessage(), "|";
}
var_dump(count($exact), count($child), count($invalid));
"#;
    assert_eq!(
        run_php(source),
        "diag:Indirect modification of overloaded element of ArrayObject has no effect|ArgumentCountError:ChildArrayObject::offsetGet(): Argument #1 ($key) not passed|TypeError:Cannot access offset of type array on ArrayObject|TypeError:Cannot unset offset of type array on ArrayObject|int(0)\nint(0)\nint(0)\n"
    );
}

#[test]
fn anonymous_array_access_errors_use_the_public_interface_name() {
    let source = r#"<?php
$container = new class implements ArrayAccess {
    public function offsetExists($key): bool { return true; }
    public function offsetGet($key): mixed { throw new RuntimeException('stop'); }
    public function offsetSet($key, $value): void {}
    public function offsetUnset($key): void {}
};
try { $container['key'] += 1; } catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage();
}
"#;
    assert_eq!(
        run_php(source),
        "Error:Cannot use object of type ArrayAccess@anonymous as array"
    );
}

#[test]
fn array_append_reference_preserves_aliases_cycles_and_source_evaluation() {
    assert_eq!(
        run_php(
            r#"<?php
$source = 1;
$items = [];
$items[] =& $source;
$copy = $items;
$source = 2;
$copy[0] = 3;
echo $source, ':', $items[0], ':', $copy[0], "\n";

$calls = 0;
function choose_slot(&$calls) {
    echo 'key>';
    return $calls++;
}
$values = [4];
$references = [];
$references[] =& $values[choose_slot($calls)];
$values[0] = 5;
echo $references[0], ':', $calls, "\n";

$recursive = [];
$recursive[] =& $recursive;
var_dump($recursive);
"#,
        ),
        "3:3:3\nkey>5:1\narray(1) {\n  [0]=>\n  *RECURSION*\n}\n"
    );
}

#[test]
fn array_append_reference_keeps_the_globals_compile_prohibition() {
    let error = run_php_expect_error("<?php $source = 1; $GLOBALS[] =& $source;");
    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Cannot append to $GLOBALS on line 1"
    ));
}

#[test]
fn list_assignment_supports_array_append_targets() {
    assert_eq!(
        run_php(
            "<?php $values = ['before']; [, $class, $values[], $wakeup] = [null, 'Probe', 42, 7]; echo $class, ':', implode(',', $values), ':', $wakeup;"
        ),
        "Probe:before,42:7"
    );
}

#[test]
fn destructuring_assigns_to_this_property_targets() {
    assert_eq!(
        run_php(
            r#"<?php
class AssignmentSink {
    public string $left = '';
    public string $right = '';
    public string $slot = 'right';
    public array $bag = [];

    public function fill(): void {
        [$this->left, $this->bag['entry']] = ['alpha', 'nested'];
        list($this->{$this->slot}) = ['dynamic'];
        echo $this->left, '|', $this->right, '|', $this->bag['entry'], "\n";
    }
}

$sink = new AssignmentSink();
$sink->fill();
echo $sink->left, '|', $sink->right, '|', $sink->bag['entry'];
"#,
        ),
        "alpha|dynamic|nested\nalpha|dynamic|nested"
    );
}
