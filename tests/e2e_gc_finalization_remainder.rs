mod common;

use common::run_php;

#[test]
fn destructor_created_generations_have_one_bounded_rerun_per_collection() {
    assert_eq!(
        run_php(
            r#"<?php
class DeferredGeneration {
    public $self;
    function __construct(public $number) { $this->self = $this; }
    function __destruct() {
        echo 'd', $this->number, ':', (int) gc_status()['running'], '|';
        if ($this->number < 5) new DeferredGeneration($this->number + 1);
    }
}
gc_disable(); new DeferredGeneration(0);
for ($i = 0; $i < 4; $i++) {
    echo 'r', $i, '|'; $count = gc_collect_cycles();
    echo 'c', $count, ':', gc_status()['runs'], '|';
}
"#
        ),
        "r0|d0:1|d1:1|c1:2|r1|d2:1|d3:1|c2:4|r2|d4:1|d5:1|c2:6|r3|c1:7|"
    );
}

#[test]
fn plain_cycles_created_by_destructors_are_reclaimed_during_the_rerun() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainCycleProducer {
    public $self;
    function __construct() { $this->self = $this; }
    function __destruct() {
        global $weak;
        echo 'parent|'; $child = new stdClass; $child->self = $child;
        $weak = WeakReference::create($child);
    }
}
new PlainCycleProducer;
$first = gc_collect_cycles(); echo 'first:', $first, ':', (int)($weak->get() === null), '|';
$second = gc_collect_cycles(); echo 'second:', $second, ':', (int)($weak->get() === null), '|';
"#
        ),
        "parent|first:2:1|second:0:1|"
    );
}

#[test]
fn destructor_release_of_an_external_root_is_observed_in_the_same_collection() {
    assert_eq!(
        run_php(
            r#"<?php
class ExternalRootReleaser {
    public $self;
    function __construct() { $this->self = $this; }
    function __destruct() { echo 'release|'; unset($GLOBALS['root']); }
}
$root = new stdClass; $root->self = $root; $weak = WeakReference::create($root);
new ExternalRootReleaser;
$first = gc_collect_cycles(); echo 'first:', $first, ':', (int)($weak->get() === null), '|';
$second = gc_collect_cycles(); echo 'second:', $second, ':', (int)($weak->get() === null), '|';
"#
        ),
        "release|first:2:1|second:0:1|"
    );
}

#[test]
fn resurrection_and_new_destructor_cycles_keep_independent_lifetimes() {
    assert_eq!(
        run_php(
            r#"<?php
class SavedGeneration {
    public $self;
    function __construct(public $number) { $this->self = $this; }
    function __destruct() {
        echo 'd', $this->number, '|';
        if ($this->number === 0) { $GLOBALS['saved'] = $this; new SavedGeneration(1); }
    }
}
new SavedGeneration(0);
$count = gc_collect_cycles(); echo 'first:', $count, ':', (int)isset($saved), '|';
unset($saved); echo 'second:', gc_collect_cycles(), '|';
"#
        ),
        "d0|d1|first:0:1|second:2|"
    );
}

#[test]
fn recursive_collection_and_destructor_exceptions_preserve_following_roots() {
    assert_eq!(
        run_php(
            r#"<?php
class ThrowingGeneration {
    public $self;
    function __construct(public $number) { $this->self = $this; }
    function __destruct() {
        echo 'd', $this->number, ':', gc_collect_cycles(), '|';
        if ($this->number < 2) new ThrowingGeneration($this->number + 1);
        if ($this->number === 0) throw new Exception('retirement');
    }
}
new ThrowingGeneration(0);
try { echo 'first:', gc_collect_cycles(), '|'; }
catch (Exception $e) { echo 'caught:', $e->getMessage(), '|'; }
echo 'after:', (int)gc_status()['running'], '|';
echo 'second:', gc_collect_cycles(), '|third:', gc_collect_cycles(), '|';
"#
        ),
        "first:d0:0|d1:0|caught:retirement|after:0|second:d2:0|2|third:0|"
    );
}

#[test]
fn an_array_cycle_created_in_a_destructor_is_not_lost() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrayCycleProducer {
    public $self;
    function __construct() { $this->self = $this; }
    function __destruct() { echo 'drop|'; $array = []; $array[] =& $array; }
}
new ArrayCycleProducer;
echo 'first:', gc_collect_cycles(), '|second:', gc_collect_cycles(), '|';
"#
        ),
        "first:drop|2|second:0|"
    );
}

#[test]
fn a_new_cycle_with_an_external_root_is_not_prematurely_retired() {
    assert_eq!(
        run_php(
            r#"<?php
class RootedGeneration {
    public $self;
    function __construct(public $number) { $this->self = $this; }
    function __destruct() {
        echo 'd', $this->number, '|';
        if ($this->number === 0) $GLOBALS['saved'] = new RootedGeneration(1);
    }
}
new RootedGeneration(0);
echo 'first:', gc_collect_cycles(), '|second:', gc_collect_cycles(), '|';
unset($saved); echo 'third:', gc_collect_cycles(), '|';
"#
        ),
        "first:d0|1|second:0|third:d1|1|"
    );
}

#[test]
fn a_destructor_created_closure_cycle_releases_its_bound_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
class ClosureCycleProducer {
    public $self;
    function __construct() { $this->self = $this; }
    function __destruct() {
        global $weak;
        echo 'drop|'; $closure = function () use (&$closure) {};
        $weak = WeakReference::create($closure);
    }
}
new ClosureCycleProducer;
$first = gc_collect_cycles(); echo 'first:', $first, ':', (int)($weak->get() === null), '|';
$second = gc_collect_cycles(); echo 'second:', $second, ':', (int)($weak->get() === null), '|';
"#
        ),
        "drop|first:2:1|second:0:1|"
    );
}
