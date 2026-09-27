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

/// One property site executed for many receiver classes (a base-class method
/// reading and writing private, protected, public and typed properties, with
/// subclasses shadowing or redeclaring them) refills its inline cache from the
/// polymorphic memo; results must stay exactly PHP's. The tail exercises
/// preg_replace group reuse across iterations. Verified against reference PHP.
#[test]
fn polymorphic_property_sites_keep_php_semantics() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class Base {
    private $secret = 'base'; protected $shared = 0; public $pub = 0; protected int $typed = 0; private array $items = [];
    function tick($v) { $this->shared += 1; $this->pub = $v; $this->typed = (int) $v; $this->items[] = $v; $this->secret = $v . static::class; return [$this->shared, $this->pub, $this->typed, count($this->items), $this->secret, $this->readSecret()]; }
    function readSecret() { return $this->secret; }
}
class A extends Base {}
class B extends Base { private $secret = 'B-own'; function readSecret() { return $this->secret; } }
class C extends Base { protected int $typed = 5; }
class D extends Base { public $pub = 'd'; }
class E extends Base { function tick($v) { $r = parent::tick($v); $this->shared *= 2; return $r; } }
$objs = [new A, new B, new C, new D, new E, new A, new B];
for ($round = 0; $round < 3; $round++) {
    foreach ($objs as $i => $o) { echo $i, ':', json_encode($o->tick("v$round")), '|'; }
    echo "\n";
}
$s = "aXbXcXd"; for ($i = 0; $i < 3; $i++) echo preg_replace('/(X)(.)/', '[$2$1]', $s), preg_replace('/(?<n>\w)(?=X)/', '<$1>', $s), preg_replace('/(a)|(b)/', '{$1|$2}', $s), "\n";
"#
        ),
        "0:[1,\"v0\",0,1,\"v0A\",\"v0A\"]|1:[1,\"v0\",0,1,\"v0B\",\"B-own\"]|2:[1,\"v0\",0,1,\"v0C\",\"v0C\"]|3:[1,\"v0\",0,1,\"v0D\",\"v0D\"]|4:[1,\"v0\",0,1,\"v0E\",\"v0E\"]|5:[1,\"v0\",0,1,\"v0A\",\"v0A\"]|6:[1,\"v0\",0,1,\"v0B\",\"B-own\"]|\n0:[2,\"v1\",0,2,\"v1A\",\"v1A\"]|1:[2,\"v1\",0,2,\"v1B\",\"B-own\"]|2:[2,\"v1\",0,2,\"v1C\",\"v1C\"]|3:[2,\"v1\",0,2,\"v1D\",\"v1D\"]|4:[3,\"v1\",0,2,\"v1E\",\"v1E\"]|5:[2,\"v1\",0,2,\"v1A\",\"v1A\"]|6:[2,\"v1\",0,2,\"v1B\",\"B-own\"]|\n0:[3,\"v2\",0,3,\"v2A\",\"v2A\"]|1:[3,\"v2\",0,3,\"v2B\",\"B-own\"]|2:[3,\"v2\",0,3,\"v2C\",\"v2C\"]|3:[3,\"v2\",0,3,\"v2D\",\"v2D\"]|4:[7,\"v2\",0,3,\"v2E\",\"v2E\"]|5:[3,\"v2\",0,3,\"v2A\",\"v2A\"]|6:[3,\"v2\",0,3,\"v2B\",\"B-own\"]|\na[bX][cX][dX]<a>X<b>X<c>Xd{a|}X{|b}XcXd\na[bX][cX][dX]<a>X<b>X<c>Xd{a|}X{|b}XcXd\na[bX][cX][dX]<a>X<b>X<c>Xd{a|}X{|b}XcXd\n"
    );
}

/// Slow constant lookups are memoized per spelling, including misses, and
/// every define() clears the memo: namespaced fallback, define() after a
/// failed lookup, defined()/constant() must keep PHP's answers. The tail
/// covers reference-foreach cursor copies and weak references alive at
/// shutdown with nothing left to destruct. Verified against reference PHP.
#[test]
fn constant_lookup_memo_and_cursor_copies_follow_php() {
    assert_eq!(
        run_php(
            r#"<?php
namespace App;
echo PHP_EOL === "\n" ? 'eol|' : 'x|', E_ALL, '|', \PHP_INT_SIZE, '|', defined('LATER') ? 'd' : 'u', '|', defined('App\\LATER') ? 'd' : 'u', '|';
for ($i = 0; $i < 3; $i++) { echo defined('LATER') ? 'D' : 'U'; }
define('LATER', 42); echo '|', LATER, '|', \LATER, '|', constant('LATER'), '|', defined('LATER') ? 'D' : 'U', '|', defined('App\\LATER') ? 'D' : 'U', "\n";
const LOCAL = 7; echo LOCAL, '|', \App\LOCAL, '|', constant('App\\LOCAL'), '|', defined('LOCAL') ? 'D' : 'U', '|';
try { echo MISSING; } catch (\Error $e) { echo get_class($e), ':', $e->getMessage(), '|'; }
define('MISSING', 'now'); echo MISSING, '|', \MISSING, "\n";
define('App\\NS', 'ns'); echo NS, '|', \App\NS, '|', constant('App\\NS'), '|', defined('NS') ? 'D' : 'U', "\n";
// reference foreach with copies of the iterated array
$a = [1, 2, 3, 4]; $copies = [];
foreach ($a as $k => &$v) { $copies[] = $a; $v *= 10; if ($k === 1) { $b = $a; $b[] = 99; } }
unset($v); echo json_encode($a), json_encode($b), json_encode(array_map('count', $copies)), "\n";
$m = [[1, 2], [3, 4]]; foreach ($m as &$row) { foreach ($row as &$cell) { $cell += 1; $snapshot = $m; } unset($cell); } unset($row); echo json_encode($m), json_encode($snapshot), "\n";
// weak refs alive at shutdown, no destructors
$w = \WeakReference::create($obj = new \stdClass); $map = new \WeakMap; $map[$obj] = 1; echo count($map), "|end\n";
"#
        ),
        "eol|30719|8|u|u|UUU|42|42|42|D|U\n7|7|7|U|Error:Undefined constant \"App\\MISSING\"|now|now\nns|ns|ns|U\n[10,20,30,40][10,20,3,4,99][4,4,4,4]\n[[2,3],[4,5]][[2,3],[4,5]]\n1|end\n"
    );
}

/// The reference-foreach cursor registry indexes arrays by identity: copies
/// made while a cursor is live inherit its position, copies of a formerly
/// iterated array without a live cursor stop carrying the flag, splices adjust
/// both the iterated array and remembered copies, and generators keep their
/// cursor across suspensions.
#[test]
fn reference_foreach_cursor_index_follows_php() {
    assert_eq!(
        run_php(
            r#"<?php
function f1() {
    $a = [1, 2, 3, 4];
    $out = [];
    foreach ($a as $k => &$v) {
        if ($k === 1) { $b = $a; $b[] = 5; $a = $b; }
        $v *= 10;
        $out[] = $v;
    }
    unset($v);
    return [$a, $out];
}
function f2() {
    $a = [1, 2, 3, 4];
    foreach ($a as &$v) { $v++; }
    unset($v);
    $c = $a;            // flagged source, no live cursor
    $c[] = 9;
    array_splice($c, 1, 1);
    $d = $c;
    foreach ($d as $k => &$w) { if ($k === 1) { array_splice($d, 0, 1); } $w += 100; }
    unset($w);
    return [$a, $c, $d];
}
function f3() {
    $a = ['x' => [1, 2], 'y' => [3]];
    $seen = [];
    foreach ($a as $k => &$v) {
        $copy = $a;
        $copy[$k][] = 'c';
        $v[] = 'r';
        $seen[] = count($copy[$k]);
        if ($k === 'x') { $a = $copy; }
    }
    unset($v);
    return [$a, $seen];
}
function f4() {
    $a = range(1, 6);
    $res = [];
    foreach ($a as $k => &$v) {
        if ($k === 2) { unset($a[3]); $a[] = 7; }
        $res[] = $v;
    }
    unset($v);
    return [$a, $res];
}
function f5() {
    $rows = [[1], [2], [3]];
    foreach ($rows as &$row) {
        foreach ($row as &$cell) { $cell *= 2; }
        unset($cell);
        $snapshot = $rows;
        $snapshot[] = [0];
        $row[] = count($snapshot);
    }
    unset($row);
    return $rows;
}
foreach (['f1', 'f2', 'f3', 'f4', 'f5'] as $fn) { echo $fn, ': ', json_encode($fn()), "\n"; }
$gen = (function () { $a = [1, 2, 3]; foreach ($a as &$v) { $b = $a; $b[0] = 40; yield $v; if ($v === 2) { $a = $b; } } })();
foreach ($gen as $x) echo $x, ' ';
echo "\n";
"#
        ),
        r#"f1: [[10,20,30,40,50],[10,20,30,40,50]]
f2: [[2,3,4,5],[2,4,5,9],[109]]
f3: [{"x":[1,2,"c","r"],"y":[3,"c","r"]},[4,3]]
f4: [{"0":1,"1":2,"2":3,"4":5,"5":6,"6":7},[1,2,3,5,6,7]]
f5: [[2,4],[4,4],[6,4]]
40 2 3 
"#
    );
}
