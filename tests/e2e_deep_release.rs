mod common;

use common::run_php;

#[test]
fn shared_reference_and_native_edges_preserve_final_leaf_release() {
    assert_eq!(
        run_php(
            r#"<?php
gc_disable();
class ObservedReleaseLeaf {
    public function __destruct() { echo 'leaf|'; }
}
function referencePair($value) { return [&$value, &$value]; }
$node = new ObservedReleaseLeaf;
$weak = WeakReference::create($node);
for ($i = 0; $i < 600; $i++) {
    if ($i % 4 === 0) { $node = referencePair($node); }
    elseif ($i % 4 === 1) { $node = (object) ['next' => $node]; }
    elseif ($i % 4 === 2) { $node = static fn () => $node; }
    else { $node = [$node, $node]; }
}
$outside = new ArrayIterator([$node]);
unset($node);
echo $weak->get() === null ? 'lost|' : 'held|';
unset($outside);
echo $weak->get() === null ? 'cleared' : 'retained';
"#
        ),
        "held|leaf|cleared"
    );
}

#[test]
fn deep_destructor_chain_runs_each_callback_in_parent_first_order() {
    assert_eq!(
        run_php(
            r#"<?php
gc_disable();
class ReleaseNode {
    public function __construct(public ?self $next, public int $index) {}
    public function __destruct() {
        if ($this->index !== $GLOBALS['expected']--) { echo 'out-of-order|'; }
        $GLOBALS['released']++;
    }
}
$expected = 2047;
$released = 0;
$root = null;
for ($i = 0; $i < 2048; $i++) { $root = new ReleaseNode($root, $i); }
unset($root);
echo $released, ':', $expected, "\n";
"#,
        ),
        "2048:-1\n"
    );
}

#[test]
fn destructor_created_graphs_survive_resurrection_aliases_and_exceptions() {
    assert_eq!(
        run_php(
            r#"<?php
gc_disable();
class ReleaseLeaf {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'leaf:', $this->name, '|'; }
}
function deepReleasePayload(string $name): mixed {
    $value = $name === 'throw' ? new stdClass : new ReleaseLeaf($name);
    for ($i = 0; $i < 3000; $i++) {
        if ($i % 3 === 0) { $value = [$value]; }
        elseif ($i % 3 === 1) { $value = (object) ['child' => $value]; }
        else { $value = static fn () => $value; }
    }
    return $value;
}
class ReleaseOwner {
    public mixed $payload = null;
    public function __construct(public string $mode) {}
    public function __destruct() {
        echo 'owner:', $this->mode, '|';
        $this->payload = deepReleasePayload($this->mode);
        if ($this->mode === 'resurrect') { $GLOBALS['saved'] = $this; }
        if ($this->mode === 'alias') { $GLOBALS['alias'] = $this->payload; }
        if ($this->mode === 'throw') { throw new Exception('release'); }
    }
}
$owner = new ReleaseOwner('resurrect');
$weak = WeakReference::create($owner);
unset($owner);
echo (int) ($weak->get() === $saved), '|';
unset($saved);
echo (int) ($weak->get() === null), '|';
$owner = new ReleaseOwner('alias');
unset($owner);
echo 'retained|';
unset($alias);
try { $owner = new ReleaseOwner('throw'); unset($owner); }
catch (Throwable $error) { echo 'caught:', $error->getMessage(), '|'; }
echo 'done';
"#,
        ),
        concat!(
            "owner:resurrect|1|leaf:resurrect|1|",
            "owner:alias|retained|leaf:alias|",
            "owner:throw|caught:release|done"
        )
    );
}

#[test]
fn suspended_destructor_releases_a_graph_added_while_parked() {
    assert_eq!(
        run_php(
            r#"<?php
gc_disable();
class ParkedReleaseLeaf {
    public function __destruct() { echo 'leaf|'; }
}
class ParkedReleaseOwner {
    public mixed $payload = null;
    public function __destruct() { echo 'owner:', Fiber::suspend('paused'), '|'; }
}
$fiber = new Fiber(function () {
    $root = new ParkedReleaseOwner;
    $GLOBALS['weak'] = WeakReference::create($root);
    unset($root);
    echo 'after|';
});
echo $fiber->start(), '|';
$value = new ParkedReleaseLeaf;
for ($i = 0; $i < 3000; $i++) { $value = [$value]; }
$weak->get()->payload = $value;
unset($value);
$fiber->resume('resumed');
echo (int) ($weak->get() === null), ':', (int) $fiber->isTerminated();
"#,
        ),
        "owner:paused|resumed|leaf|after|1:1"
    );
}
