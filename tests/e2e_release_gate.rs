mod common;

use common::run_php;

/// Release planning is skipped while no live value can run PHP code on drop.
/// The exact live count must therefore cover every way such a value appears:
/// constructed, cloned, created through Reflection, inherited destructors,
/// Generators, nested arrays and cycles left for shutdown. Verified against
/// reference PHP.
#[test]
fn destructors_still_run_when_release_planning_is_gated() {
    assert_eq!(
        run_php(
            r#"<?php
class D { function __construct(public $n) {} function __destruct() { echo "d:{$this->n}|"; } }
class P { function __construct(public $n) {} }
class Holder { public $items = []; private $priv = []; function set($v) { $this->items = $v; } function setPriv($v) { $this->priv = $v; } }
// 1. replaced property array holding destructor objects
$h = new Holder;
$h->set([new D('a'), new P(1)]); echo "1|"; $h->set([new P(2)]); echo "2|";
$h->setPriv([new D('b')]); $h->setPriv([]); echo "3|";
// 2. destructor object created via clone and via reflection
$d = new D('c'); $e = clone $d; $e->n = 'c2'; unset($e); echo "4|"; unset($d); echo "5|";
$r = (new ReflectionClass(D::class))->newInstanceWithoutConstructor(); $r->n = 'r'; unset($r); echo "6|";
// 3. generator in a replaced array
function gen() { try { yield 1; yield 2; } finally { echo "gfin|"; } }
$h->set([gen()]); $h->items[0]->current(); $h->set([]); echo "7|";
// 4. nested arrays with objects: replaced sole-owned array of arrays
$h->set([[new D('n1')], [new P(3), [new D('n2')]]]); $h->set(null); echo "8|";
// 5. class declared later with destructor via inheritance
class Base { function __destruct() { echo "base:" . static::class . "|"; } }
class Child extends Base {}
$h->set([new Child]); $h->set([]); echo "9|";
// 6. cycle with destructor left for shutdown
class Cyc { public $other; function __destruct() { echo "cyc|"; } }
$x = new Cyc; $y = new Cyc; $x->other = $y; $y->other = $x; unset($x, $y);
// 7. stream resource
$f = fopen('php://memory', 'w+'); fwrite($f, 'x'); $h->set([$f]); $h->set([]); echo "10|";
echo "end\n";
"#
        ),
        "1|d:a|2|d:b|3|d:c2|4|d:c|5|d:r|6|gfin|7|d:n1|d:n2|8|base:Child|9|10|end\ncyc|cyc|"
    );
}

/// Weak references and maps attach release work to otherwise plain objects.
/// The gate must open as soon as any such work exists, and the weak runtime's
/// per-identity checks must stay exact after the indexed rewrite. Verified
/// against reference PHP.
#[test]
fn weak_release_work_reopens_the_gate() {
    assert_eq!(
        run_php(
            r#"<?php
class P { function __construct(public $n) {} }
class D { function __construct(public $n) {} function __destruct() { echo "d:{$this->n}|"; } }
class H { public $items = []; function set($v) { $this->items = $v; } }
$h = new H;
// weak reference to an object held only by a replaced property array
$o = new P('w1'); $r = WeakReference::create($o); $h->set([$o]); unset($o);
echo var_export($r->get() !== null, true), '|'; $h->set([]); echo var_export($r->get(), true), "|1\n";
// WeakMap keyed by objects in a replaced array
$m = new WeakMap; $a = new P('a'); $b = new D('b'); $m[$a] = 'va'; $m[$b] = 'vb';
$h->set([$a, $b]); unset($a, $b); echo count($m), '|'; $h->set([]); echo count($m), "|2\n";
// WeakReference target replaced while the WeakReference object itself dies first
$o2 = new P('w2'); $r2 = WeakReference::create($o2); unset($r2); $h->set([$o2]); unset($o2); $h->set([]); echo "3\n";
// WeakMap cloned, then keys die
$m2 = new WeakMap; $k = new P('k'); $m2[$k] = 1; $m3 = clone $m2; $h->set([$k]); unset($k); echo count($m2), count($m3), '|'; $h->set([]); echo count($m2), count($m3), "|4\n";
// WeakMap object itself dropped while holding destructor values
$m4 = new WeakMap; $k2 = new P('k2'); $m4[$k2] = new D('mv'); $h->set([$m4]); unset($m4); $h->set([]); echo "5\n";
// nested: array of arrays with weak-tracked keys
$m5 = new WeakMap; $k3 = new P('k3'); $m5[$k3] = 1; $h->set([[[$k3]]]); unset($k3); $h->set(null); echo count($m5), "|6\n";
echo "end\n";
"#
        ),
        "true|NULL|1\n2|d:b|0|2\n3\n11|00|4\nd:mv|5\n0|6\nend\n"
    );
}
