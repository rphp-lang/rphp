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
