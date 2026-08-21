/// E2E tests: arrays — literals, access, assignment, push, nested, functions.
mod common;
use common::{run_php, run_php_expect_error};

include!("e2e_arrays/basic_operations.rs");

include!("e2e_arrays/copy_on_write.rs");

include!("e2e_arrays/mutation_and_hot_paths.rs");

include!("e2e_arrays/unpack_semantics.rs");

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
