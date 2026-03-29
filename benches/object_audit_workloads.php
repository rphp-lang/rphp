<?php
// Object/method coverage audit workloads for hot tier Etapa E
// Each section tests a different object/method pattern

// ── 1. Simple method recursion ──
class TreeNode {
    public $val;
    public $left;
    public $right;
    public function __construct($v, $l, $r) {
        $this->val = $v;
        $this->left = $l;
        $this->right = $r;
    }
    public function sum() {
        $s = $this->val;
        if ($this->left != null) $s = $s + $this->left->sum();
        if ($this->right != null) $s = $s + $this->right->sum();
        return $s;
    }
}
$t = new TreeNode(1,
    new TreeNode(2, new TreeNode(3, null, null), new TreeNode(4, null, null)),
    new TreeNode(5, new TreeNode(6, null, null), new TreeNode(7, null, null))
);
echo "tree_sum=" . $t->sum() . "\n";

// ── 2. Property read-heavy (Counter pattern) ──
class Accum {
    public $total = 0;
    public function add($x) { $this->total = $this->total + $x; return $this; }
    public function get() { return $this->total; }
}
$a = new Accum();
for ($i = 0; $i < 1000; $i++) { $a->add($i); }
echo "accum=" . $a->get() . "\n";

// ── 3. Method chain (fluent API) ──
class Builder {
    public $val = 0;
    public function inc() { $this->val = $this->val + 1; return $this; }
    public function dbl() { $this->val = $this->val * 2; return $this; }
    public function result() { return $this->val; }
}
$b = new Builder();
for ($i = 0; $i < 500; $i++) { $b->inc()->dbl(); }
echo "builder=" . $b->result() . "\n";

// ── 4. Service/helper chain (multi-object) ──
class Adder {
    public function apply($x) { return $x + 1; }
}
class Doubler {
    public function apply($x) { return $x * 2; }
}
$adder = new Adder();
$doubler = new Doubler();
$r = 0;
for ($i = 0; $i < 1000; $i++) {
    $r = $doubler->apply($adder->apply($r));
}
echo "service=" . $r . "\n";

// ── 5. Pure method recursion (no property access) ──
class MathHelper {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$m = new MathHelper();
echo "method_fib=" . $m->fib(20) . "\n";

// ── 6. Static method calls ──
class Calculator {
    public static function add($a, $b) { return $a + $b; }
    public static function mul($a, $b) { return $a * $b; }
}
$r = 0;
for ($i = 0; $i < 1000; $i++) {
    $r = Calculator::add($r, Calculator::mul($i, 2));
}
echo "static=" . $r . "\n";

// ── 7. Mixed: object + scalar computation ──
class Stats {
    public $count = 0;
    public $sum = 0;
    public function record($v) {
        $this->count = $this->count + 1;
        $this->sum = $this->sum + $v;
    }
    public function avg() {
        if ($this->count == 0) return 0;
        return $this->sum;
    }
}
$st = new Stats();
for ($i = 0; $i < 1000; $i++) { $st->record($i * 2 + 1); }
echo "stats=" . $st->avg() . "\n";

// ── 8. Closure + object mix ──
class Transform {
    public $fn;
    public function __construct($f) { $this->fn = $f; }
    public function apply($x) {
        $f = $this->fn;
        return $f($x);
    }
}
$t = new Transform(function($x) { return $x + 1; });
$r = 0;
for ($i = 0; $i < 1000; $i++) { $r = $t->apply($r); }
echo "closure_obj=" . $r . "\n";
