mod common;

use common::run_php;

/// Non-public declared properties read from a scope fixed by the executing
/// function are served by the property inline cache. The verdict must follow
/// PHP for shadowed parent privates, rebound closures sharing one op array,
/// trait methods composed into different classes, `__get` fallbacks and
/// readonly modification through a private property.
#[test]
fn scoped_property_reads_follow_php_visibility() {
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private $p = 'A-private';
    protected $q = 'A-protected';
    public $r = 'A-public';
    function readP() { return $this->p; }
    function readQ() { return $this->q; }
    function readAll($o) { return $o->p . '|' . $o->q . '|' . $o->r; }
    function inc() { $this->cnt++; $this->arr[] = 1; return $this->cnt . ':' . count($this->arr); }
    private $cnt = 0; private $arr = [];
}
class B extends A {
    private $p = 'B-private';
    function readPB() { return $this->p; }
    function readQB() { return $this->q; }
    function readParentP() { return $this->p; }
}
class C { private $p = 'C-private'; function __get($n) { return "magic:$n"; } }
trait T { function readT() { return $this->tp; } }
class D { use T; private $tp = 'D-tp'; }
class E { use T; private $tp = 'E-tp'; }
class F { private $tp = 'F-tp'; }

$a = new A; $b = new B;
for ($i = 0; $i < 3; $i++) {
    echo $a->readP(), '|', $a->readQ(), '|', $b->readP(), '|', $b->readPB(), '|', $b->readQB(), '|', $b->readParentP(), "\n";
    echo $a->readAll($a), '|', $a->readAll($b), "\n";
    echo $a->inc(), '|', $b->inc(), "\n";
    echo (new D)->readT(), '|', (new E)->readT(), "\n";
}
// same closure op_array, rebound to different scopes
$f = function () { return $this->p; };
$fa = Closure::bind($f, $a, A::class);
$fb = Closure::bind($f, $b, B::class);
$fb2 = Closure::bind($f, $b, A::class);
for ($i = 0; $i < 3; $i++) { echo $fa(), '|', $fb(), '|', $fb2(), "\n"; }
// method reading private of unrelated object -> __get / error
$c = new C;
try { echo $a->readAll($c), "\n"; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { echo (new D)->readT(), '|'; $g = Closure::bind(function () { return $this->tp; }, new F, F::class); echo $g(), "\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
// protected read from outside -> error
try { echo $a->q; } catch (Error $e) { echo $e->getMessage(), "\n"; }
// readonly private modify via method
class R { public function __construct(private readonly array $items = [1]) {} function push() { $this->items[] = 2; } function get() { return $this->items; } }
$r = new R;
try { $r->push(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo json_encode($r->get()), "\n";
"#
        ),
        "A-private|A-protected|A-private|B-private|A-protected|B-private\nA-private|A-protected|A-public|A-private|A-protected|A-public\n1:1|1:1\nD-tp|E-tp\nA-private|A-protected|A-private|B-private|A-protected|B-private\nA-private|A-protected|A-public|A-private|A-protected|A-public\n2:2|2:2\nD-tp|E-tp\nA-private|A-protected|A-private|B-private|A-protected|B-private\nA-private|A-protected|A-public|A-private|A-protected|A-public\n3:3|3:3\nD-tp|E-tp\nA-private|B-private|A-private\nA-private|B-private|A-private\nA-private|B-private|A-private\nmagic:p|magic:q|magic:r\nD-tp|F-tp\nCannot access protected property A::$q\nCannot indirectly modify readonly property R::$items\n[1]\n"
    );
}

/// `strlen($this->prop)` fused into the cached property read must count PHP
/// bytes: a stored `\xff` byte is wider in the internal representation.
#[test]
fn fused_strlen_of_cached_property_counts_php_bytes() {
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private $p = 13; private $b = "one\ntwo\n\xfftail"; public $pp = 13; public $pb = "one\ntwo\n\xfftail";
    function f() { return $this->p >= strlen($this->b); }
    function g() { $n = strlen($this->b); return $this->p >= $n; }
    function h() { return $this->pp >= strlen($this->pb); }
    function i() { return strlen($this->b); }
    function j() { $x = $this->b; return strlen($x); }
    function k() { return $this->p; }
}
$a = new A;
for ($r = 0; $r < 3; $r++) echo (int)$a->f(), (int)$a->g(), (int)$a->h(), $a->i(), $a->j(), $a->k(), '|';
echo "\n";
"#
        ),
        "111131313|111131313|111131313|\n"
    );
}
