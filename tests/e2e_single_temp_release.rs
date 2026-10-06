mod common;
use common::run_php;

fn with_padding(body: &str, padding: usize) -> String {
    let mut source = String::from("<?php function exercise() {");
    for index in 0..padding {
        source.push_str(&format!("$padding{index} = {index};"));
    }
    source.push_str(body);
    source.push_str("} exercise();");
    source
}

#[test]
fn shared_array_temporaries_keep_nested_objects_and_cow_owners_alive() {
    let body = r#"
    class SharedTempLeaf {
        function __construct(public $name) {}
        function __destruct() { echo 'drop:', $this->name, '|'; }
    }
    function sharedTempValue($value) { return $value; }
    $leaf = new SharedTempLeaf('shared');
    $weak = WeakReference::create($leaf);
    $first = [$leaf, 1];
    $second = $first;
    unset($leaf);
    echo sharedTempValue($first)[1], '|';
    $first[1] = 2;
    echo sharedTempValue($first)[1], ':', sharedTempValue($second)[1], '|';
    unset($first);
    echo 'live:', (int)($weak->get() !== null), '|';
    unset($second);
    echo 'gone:', (int)($weak->get() === null), '|';
    "#;
    for padding in [0, 70] {
        assert_eq!(
            run_php(&with_padding(body, padding)),
            "1|2:1|live:1|drop:shared|gone:1|",
            "{padding} padding CVs"
        );
    }
}

#[test]
fn failed_pending_call_releases_its_last_nested_object_before_catch() {
    let body = r#"
    class PendingTempLeaf {
        function __destruct() { echo 'drop:pending|'; }
    }
    function pendingTempArray() { return [new PendingTempLeaf]; }
    function pendingTempReject() { throw new Exception('blocked'); }
    function pendingTempConsume($first, $second) {}
    try { pendingTempConsume(pendingTempArray(), pendingTempReject()); }
    catch (Exception $error) { echo $error->getMessage(), '|'; }
    echo 'after|';
    "#;
    for padding in [0, 70] {
        assert_eq!(
            run_php(&with_padding(body, padding)),
            "drop:pending|blocked|after|",
            "{padding} padding CVs"
        );
    }
}

#[test]
fn callback_replacement_preserves_a_separate_owner_of_the_old_array() {
    let body = r#"
    class ReplacedTempLeaf {
        function __construct(public $name) {}
        function __destruct() { echo 'drop:', $this->name, '|'; }
    }
    function replaceTempRow($copy, &$slot) {
        $slot = [new ReplacedTempLeaf('new')];
        return count($copy);
    }
    $rows = ['row' => [new ReplacedTempLeaf('old')]];
    $held = $rows['row'];
    echo replaceTempRow($rows['row'], $rows['row']), '|after|';
    unset($held);
    unset($rows);
    echo 'done|';
    "#;
    for padding in [0, 70] {
        assert_eq!(
            run_php(&with_padding(body, padding)),
            "1|after|drop:old|drop:new|done|",
            "{padding} padding CVs"
        );
    }
}

#[test]
fn shared_object_temporary_keeps_cycle_admission_and_destructor_timing() {
    let body = r#"
    class CyclicTempLeaf {
        public $self;
        function __destruct() { echo 'drop:cycle|'; }
    }
    function cyclicTempValue($value) { return $value; }
    $owner = new CyclicTempLeaf;
    $owner->self = $owner;
    $weak = WeakReference::create($owner);
    echo 'same:', (int)(cyclicTempValue($owner) === $owner), '|';
    unset($owner);
    echo 'before:', (int)($weak->get() !== null), '|';
    echo gc_collect_cycles(), '|gone:', (int)($weak->get() === null), '|';
    "#;
    for padding in [0, 70] {
        assert_eq!(
            run_php(&with_padding(body, padding)),
            "same:1|before:1|drop:cycle|1|gone:1|",
            "{padding} padding CVs"
        );
    }
}
