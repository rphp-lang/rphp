mod common;

use common::run_php;

/// `$this->items[$k] = $v` on an initialized typed `array` property is an
/// ordinary cached write-back; uninitialized, nullable and untyped storage,
/// copies taken mid-loop, string appends and type errors keep PHP semantics.
#[test]
fn typed_array_property_write_backs_hit_the_cache() {
    assert_eq!(
        run_php(
            r#"<?php
class P {
    protected array $stack;
    protected array $pos = [];
    protected ?array $maybe = null;
    protected $untyped;
    public function __construct() { $this->stack = []; }
    public function run(): string {
        for ($i = 0; $i < 5; $i++) { $this->stack[$i] = $i * 2; $this->pos[] = $i; $this->untyped[$i] = "u$i"; }
        $this->maybe[] = 'm';
        $copy = $this->stack; $copy[] = 99; $this->stack[10] = 'x';
        return json_encode([$this->stack, $this->pos, $this->maybe, $this->untyped, count($copy)]);
    }
}
$p = new P; echo $p->run(), "\n"; echo $p->run(), "\n";
class Q { public array $a; public function fill() { $this->a[] = 1; return $this->a; } }
$q = new Q; echo json_encode($q->fill()), json_encode($q->fill()), "\n";
class T { public string $s = ''; public function app() { $this->s .= 'x'; return $this->s; } }
$t = new T; $t->app(); echo $t->app(), "\n";
class U { private array $m = ['a' => 1]; public function set($k, $v) { $this->m[$k] = $v; return $this->m; } }
$u = new U; $u->set('b', 2); echo json_encode($u->set('c', 3)), "\n";
try { $bad = new class { public array $a; public function go() { $this->a = 'str'; } }; $bad->go(); } catch (TypeError $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
class V { public array $a = []; public function &ref() { return $this->a; } public function go() { $r = &$this->ref(); $r[] = 1; $this->a[] = 2; return json_encode($this->a); } }
$v = new V; echo $v->go(), $v->go(), "\n";
"#
        ),
        r#"[{"0":0,"1":2,"2":4,"3":6,"4":8,"10":"x"},[0,1,2,3,4],["m"],["u0","u1","u2","u3","u4"],6]
[{"0":0,"1":2,"2":4,"3":6,"4":8,"10":"x"},[0,1,2,3,4,0,1,2,3,4],["m","m"],["u0","u1","u2","u3","u4"],7]
[1][1,1]
xx
{"a":1,"b":2,"c":3}
TypeError: Cannot assign string to property class@anonymous::$a of type array
[1,2][1,2,1,2]
"#
    );
}

/// Classes with `__set` still write declared, accessible properties directly
/// (cached), while undeclared and explicitly unset properties reach `__set`.
#[test]
fn magic_set_classes_cache_declared_property_writes() {
    assert_eq!(
        run_php(
            r#"<?php
class S {
    public $a;
    private $b;
    public $c = 5;
    public function __construct($x) { $this->a = $x; $this->b = $x + 1; $this->c = $x + 2; }
    public function __set($n, $v) { echo "SET $n\n"; }
    public function __get($n) { echo "GET $n\n"; return null; }
    public function bump() { $this->a++; $this->b .= '!'; return $this->b; }
}
$s = new S(1); echo $s->a, ' ', $s->bump(), "\n";
$s->undeclared = 3; unset($s->a); $s->a = 9; var_dump($s->a);
for ($i = 0; $i < 3; $i++) { $t = new S($i); $t->c = $i; }
echo "done\n";
"#
        ),
        r#"1 2!
SET undeclared
SET a
GET a
NULL
done
"#
    );
}

/// A scoped (non-public) read-cache entry is also write-safe unless the
/// property is readonly, an enum case or narrows its set visibility, so
/// `$this->stack[$k] = $v` and `$this->pos++` on protected/private storage
/// stay cached; readonly, asymmetric and rebound-closure writes keep PHP's
/// errors.
#[test]
fn scoped_property_modifications_follow_php_visibility() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    protected array $stack = [];
    protected int $pos = 0;
    private array $mine = [];
    public function push($v) { $this->stack[$this->pos] = $v; $this->pos++; $this->mine[] = $v; return $this; }
    public function dump() { return json_encode([$this->stack, $this->pos, $this->mine]); }
}
class Child extends Base {
    private array $mine = ['child'];
    public function pushTwice($v) { $this->stack[] = $v; $this->stack[] = $v; $this->pos += 2; $this->mine[] = $v; return $this; }
    public function mine() { return json_encode($this->mine); }
}
$b = new Base; for ($i = 0; $i < 4; $i++) { $b->push($i); } echo $b->dump(), "\n";
$c = new Child; for ($i = 0; $i < 3; $i++) { $c->push($i)->pushTwice("x$i"); } echo $c->dump(), ' ', $c->mine(), "\n";
class RO { public function __construct(public readonly array $items) {} public function bump() { try { $this->items[] = 1; } catch (Error $e) { return get_class($e) . ': ' . $e->getMessage(); } return 'ok'; } }
$r = new RO([1]); echo $r->bump(), ' ', $r->bump(), "\n";
class Asym { public private(set) array $items = []; public protected(set) int $n = 0; public function add($v) { $this->items[] = $v; $this->n++; return count($this->items) . ':' . $this->n; } }
$a = new Asym; $a->add(1); echo $a->add(2), ' ', json_encode($a->items), "\n";
try { $a->items[] = 3; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
class Str { protected string $s = ''; protected $u; public function app() { $this->s .= 'ab'; $this->u .= 'cd'; return $this->s . '|' . $this->u; } }
$s = new Str; $s->app(); echo $s->app(), "\n";
$fn = function () { $this->stack[] = 'closure'; $this->pos++; return $this->dump(); };
echo Closure::bind($fn, $b, Base::class)(), "\n";
try { echo Closure::bind($fn, $b, null)(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
"#
        ),
        r#"[[0,1,2,3],4,[0,1,2,3]]
[[0,"x0","x0",1,"x1","x1",2,"x2","x2"],9,[0,1,2]] ["child","x0","x1","x2"]
Error: Cannot indirectly modify readonly property RO::$items Error: Cannot indirectly modify readonly property RO::$items
2:2 [1,2]
Error: Cannot indirectly modify private(set) property Asym::$items from global scope
abab|cdcd
[[0,1,2,3,"closure"],5,[0,1,2,3]]
Error: Cannot access protected property Base::$stack
"#
    );
}

/// Non-public property accesses inside closures are cached per bound class
/// scope: the same closure site rebound to another object or scope, shadowed
/// privates, trait closures composed into several classes, nested closures
/// and unbound/static closures all keep PHP's results and errors.
#[test]
fn closure_sites_cache_scoped_properties_per_bound_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class A {
    protected array $items = [];
    private $secret = 'A-secret';
    protected $count = 0;
    public function reader(): Closure { return function () { return $this->secret . '/' . count($this->items) . '/' . $this->count; }; }
    public function writer(): Closure { return function ($v) { $this->items[] = $v; $this->count++; $this->secret = "A:$v"; return $this->count; }; }
    public function arrow(): Closure { return fn($x) => $this->count + $x + count($this->items); }
    public function dump() { return json_encode([$this->items, $this->secret, $this->count]); }
}
class B extends A {
    private $secret = 'B-secret';
    public function bReader(): Closure { return function () { return $this->secret; }; }
}
$a = new A; $b = new B;
$w = $a->writer(); for ($i = 0; $i < 5; $i++) { $w($i); }
$r = $a->reader(); echo $r(), ' ', $r(), ' ', $a->dump(), "\n";
$ar = $a->arrow(); echo $ar(1), $ar(2), "\n";
// Same closure site rebound to another object and another scope
$wb = Closure::bind($w, $b, A::class); for ($i = 0; $i < 3; $i++) { $wb("b$i"); }
echo $b->dump(), ' ', $b->bReader()(), "\n";
$rb = Closure::bind($r, $b, B::class); echo $rb(), "\n";   // B scope reads B::$secret
$ra = Closure::bind($r, $b, A::class); echo $ra(), "\n";   // A scope reads A::$secret
for ($i = 0; $i < 3; $i++) { echo $rb(), '|', $ra(), ' '; } echo "\n";
try { $rn = Closure::bind($r, $b, null); echo $rn(), "\n"; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
$static = static function (A $obj) { return get_class($obj); }; echo $static($a), "\n";
trait T { protected $tv = 1; public function tc(): Closure { return function () { $this->tv++; return $this->tv; }; } }
class C { use T; } class D { use T; }
$c = (new C)->tc(); $d = (new D)->tc(); echo $c(), $c(), $d(), $c(), "\n";
class E { private array $log = []; public function run() { $f = function ($m) { $this->log[] = $m; return function () { return count($this->log); }; }; $g = $f('x'); $f('y'); return $g(); } }
echo (new E)->run(), "\n";
class F { protected int $n = 0; public function go() { $inc = function () { $this->n++; }; $inc(); $inc(); return $this->n; } }
echo (new F)->go(), (new F)->go(), "\n";
class G { public readonly int $ro; public function __construct() { $this->ro = 1; } public function bad() { return function () { $this->ro = 2; }; } }
try { (new G)->bad()(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
"#
        ),
        r#"A:4/5/5 A:4/5/5 [[0,1,2,3,4],"A:4",5]
1112
[["b0","b1","b2"],"A:b2",3] B-secret
B-secret/3/3
A:b2/3/3
B-secret/3/3|A:b2/3/3 B-secret/3/3|A:b2/3/3 B-secret/3/3|A:b2/3/3 
Error: Cannot access private property B::$secret
A
2324
2
22
Error: Cannot modify readonly property G::$ro
"#
    );
}
