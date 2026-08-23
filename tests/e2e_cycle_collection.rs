mod common;

use common::run_php;

#[test]
fn explicit_collection_reclaims_object_cycles_even_when_automatic_gc_is_disabled() {
    assert_eq!(
        run_php(
            r#"<?php
class ExplicitCycleNode {
    public ?ExplicitCycleNode $peer = null;
    public function __construct(public string $name) {}
    public function __destruct() { echo "drop:$this->name\n"; }
}

gc_disable();
$left = new ExplicitCycleNode('left');
$right = new ExplicitCycleNode('right');
$left->peer = $right;
$right->peer = $left;
$weak = WeakReference::create($left);
unset($left, $right);
$collected = gc_collect_cycles();
echo 'enabled:', gc_enabled() ? 1 : 0, "\n";
echo "count:$collected\n";
echo 'weak:', $weak->get() === null ? 'null' : 'live', "\n";
"#,
        ),
        "drop:left\ndrop:right\nenabled:0\ncount:2\nweak:null\n"
    );
}

#[test]
fn arrays_references_and_closures_participate_without_counting_acyclic_children() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrayCycleOwner {
    public mixed $storage;
    public function __destruct() { echo "array-owner\n"; }
}

$owner = new ArrayCycleOwner;
$storage = ['owner' => $owner];
$owner->storage =& $storage;
$ownerWeak = WeakReference::create($owner);
unset($owner, $storage);
$arrayCount = gc_collect_cycles();
echo 'array-count:', $arrayCount, "\n";
echo 'array-weak:', $ownerWeak->get() === null ? 'null' : 'live', "\n";

class CapturedLeaf {
    public function __destruct() { echo "captured-leaf\n"; }
}
$callback = null;
$leaf = new CapturedLeaf;
$callback = function () use (&$callback, $leaf) {};
$callbackWeak = WeakReference::create($callback);
unset($callback, $leaf);
$closureCount = gc_collect_cycles();
echo 'closure-count:', $closureCount, "\n";
echo 'closure-weak:', $callbackWeak->get() === null ? 'null' : 'live', "\n";
"#,
        ),
        concat!(
            "array-owner\narray-count:2\narray-weak:null\n",
            "captured-leaf\nclosure-count:1\nclosure-weak:null\n",
        )
    );
}

#[test]
fn destructor_resurrection_defers_edge_reclamation_to_a_later_pass() {
    assert_eq!(
        run_php(
            r#"<?php
class ResurrectedCycle {
    public static ?ResurrectedCycle $saved = null;
    public ?ResurrectedCycle $self = null;
    public function __destruct() {
        echo "resurrect\n";
        self::$saved = $this;
    }
}

$object = new ResurrectedCycle;
$object->self = $object;
$weak = WeakReference::create($object);
unset($object);
$first = gc_collect_cycles();
$alive = ResurrectedCycle::$saved !== null;
echo "first:$first,live:", $alive ? 1 : 0, "\n";
ResurrectedCycle::$saved = null;
$second = gc_collect_cycles();
echo "second:$second,weak:", $weak->get() === null ? 'null' : 'live', "\n";
"#,
        ),
        "resurrect\nfirst:0,live:1\nsecond:1,weak:null\n"
    );
}

#[test]
fn weak_map_values_are_conditional_ephemeron_edges() {
    assert_eq!(
        run_php(
            r#"<?php
class EphemeronKey {
    public function __destruct() { echo "key-drop\n"; }
}
class EphemeronValue {
    public ?EphemeronKey $key = null;
    public function __destruct() { echo "value-drop\n"; }
}

$map = new WeakMap;
$key = new EphemeronKey;
$value = new EphemeronValue;
$value->key = $key;
$weak = WeakReference::create($key);
$map[$key] = $value;
unset($key, $value);
echo 'before:', count($map), "\n";
$collected = gc_collect_cycles();
echo "collected:$collected,after:", count($map), "\n";
echo 'key:', $weak->get() === null ? 'null' : 'live', "\n";

$selfMap = new WeakMap;
$mapWeak = WeakReference::create($selfMap);
$selfMap[$selfMap] = $selfMap;
unset($selfMap);
echo 'self:', gc_collect_cycles(), ',', $mapWeak->get() === null ? 'null' : 'live', "\n";
"#,
        ),
        concat!(
            "before:1\nkey-drop\nvalue-drop\n",
            "collected:2,after:0\nkey:null\nself:1,null\n",
        )
    );
}

#[test]
fn a_live_member_protects_its_cycle_until_the_last_external_root_is_removed() {
    assert_eq!(
        run_php(
            r#"<?php
class RootedCycleNode {
    public ?RootedCycleNode $peer = null;
    public function __construct(public string $name) {}
    public function __destruct() { echo "drop:$this->name\n"; }
}

$first = new RootedCycleNode('first');
$second = new RootedCycleNode('second');
$first->peer = $second;
$second->peer = $first;
$weak = WeakReference::create($first);
unset($first);
echo 'rooted:', gc_collect_cycles(), "\n";
unset($second);
$collected = gc_collect_cycles();
echo "count:$collected,weak:", $weak->get() === null ? 'null' : 'live', "\n";
echo 'again:', gc_collect_cycles(), "\n";
"#,
        ),
        "rooted:0\ndrop:second\ndrop:first\ncount:2,weak:null\nagain:0\n"
    );
}

#[test]
fn suspended_generator_values_are_cycle_edges_until_the_last_root_is_removed() {
    assert_eq!(
        run_php(
            r#"<?php
class DirectSuspendedPayload {
    public function __destruct() { echo 'direct|'; }
}
class BoundSuspendedPayload {
    public function callback() { return function () { return $this; }; }
    public function __destruct() { echo 'bound|'; }
}
function consumeSuspended(...$values) {}

$generator = null;
$generator = (function ($direct, $callback) use (&$generator) {
    consumeSuspended($generator, $callback, yield 'ready');
})(new DirectSuspendedPayload, (new BoundSuspendedPayload)->callback());
echo $generator->current(), '|';
$rooted = gc_collect_cycles();
echo "rooted:$rooted|";
$generator = null;
echo 'replaced|';
gc_collect_cycles();
echo 'released|';
echo 'again:', gc_collect_cycles(), '|';
"#,
        ),
        "ready|rooted:0|replaced|bound|direct|released|again:0|"
    );
}

#[test]
fn gc_status_reports_php_85_fields_and_updates_collection_counters() {
    assert_eq!(
        run_php(
            r#"<?php
$initial = gc_status();
echo implode(',', array_keys($initial)), "\n";
class StatusCycleNode { public $peer; }
$first = new StatusCycleNode;
$second = new StatusCycleNode;
$first->peer = $second;
$second->peer = $first;
unset($first, $second);
$queued = gc_status();
$collected = gc_collect_cycles();
$after = gc_status();
echo $initial['runs'], ':', (int) ($queued['roots'] > 0), ':',
    $collected, ':', $after['runs'], ':', $after['collected'], ':',
    $after['roots'], ':', (int) $after['running'], "\n";
echo get_debug_type($after['application_time']), ':',
    get_debug_type($after['collector_time']), ':',
    (int) ($after['application_time'] >= $initial['application_time']), ':',
    (int) ($after['collector_time'] > 0), "\n";
class StatusObservedDestructor {
    public $self;
    public function __destruct() {
        $status = gc_status();
        echo 'inside:', (int) $status['running'], ':', $status['runs'], ':',
            $status['collected'], "\n";
    }
}
$observed = new StatusObservedDestructor;
$observed->self = $observed;
unset($observed);
gc_collect_cycles();
"#,
        ),
        concat!(
            "running,protected,full,runs,collected,threshold,buffer_size,roots,",
            "application_time,collector_time,destructor_time,free_time\n",
            "0:1:2:1:2:0:0\n",
            "float:float:1:1\n",
            "inside:1:2:2\n",
        )
    );
}
