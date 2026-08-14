/// E2E tests: arrays — literals, access, assignment, push, nested, functions.
mod common;
use common::run_php;

include!("e2e_arrays/basic_operations.rs");

include!("e2e_arrays/copy_on_write.rs");

include!("e2e_arrays/mutation_and_hot_paths.rs");

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
