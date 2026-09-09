mod common;
use common::run_php;

#[test]
fn tentative_iterator_return_contract_allows_reference_key_override() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceKeyCursor extends ArrayIterator {
    #[ReturnTypeWillChange]
    public function &key() { static $key = 'retained'; return $key; }
}
foreach (new ReferenceKeyCursor([6]) as $key => $value) {
    echo $key, ':', $value, "\n";
}
"#
        ),
        "retained:6\n"
    );
}

#[test]
fn existing_native_iterator_methods_satisfy_descendant_interface_requirements() {
    assert_eq!(
        run_php(
            r#"<?php
class StoredObjects extends SplObjectStorage {}
class OrderedValues extends SplPriorityQueue {}
$object = new stdClass;
$storage = new StoredObjects;
$storage[$object] = 'info';
foreach ($storage as $key => $value) {
    echo $key, ':', $value === $object ? 'same' : 'changed', "\n";
}
$queue = new OrderedValues;
$queue->insert('first', 8);
foreach ($queue as $key => $value) { echo $key, ':', $value, "\n"; }
"#
        ),
        "0:same\n0:first\n"
    );
}

#[test]
fn untyped_iterator_implementations_emit_tentative_not_hard_errors() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($code, $message) {
    echo $code, ':', substr($message, 0, strpos($message, ' should')), "\n";
    return true;
});
class LooseCursor implements Iterator {
    public function current() { return 9; }
    public function next() {}
    public function key() { return 0; }
    public function valid() { return false; }
    public function rewind() {}
}
echo "linked\n";
"#
        ),
        concat!(
            "8192:Return type of LooseCursor::current()\n",
            "8192:Return type of LooseCursor::next()\n",
            "8192:Return type of LooseCursor::key()\n",
            "8192:Return type of LooseCursor::valid()\n",
            "8192:Return type of LooseCursor::rewind()\n",
            "linked\n",
        )
    );
}
