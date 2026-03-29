#!/bin/bash
# Object/method coverage audit — Etapa E
# Each workload isolated to get per-workload bail stats
set -e

RPHP="./target/debug/rphp"

echo "=== Object/Method Coverage Audit (Etapa E) ==="
echo ""

run_workload() {
    local name="$1"
    local code="$2"
    echo "── $name ──"
    echo "$code" | $RPHP 2>&1
    echo ""
}

run_workload "1. property read-heavy — Accum::add x1000" '<?php
class Accum {
    public $total = 0;
    public function add($x) { $this->total = $this->total + $x; return $this; }
    public function get() { return $this->total; }
}
$a = new Accum();
for ($i = 0; $i < 1000; $i++) { $a->add($i); }
echo $a->get() . "\n";'

run_workload "2. method chain — fluent Builder x500" '<?php
class Builder {
    public $val = 0;
    public function inc() { $this->val = $this->val + 1; return $this; }
    public function dbl() { $this->val = $this->val * 2; return $this; }
    public function result() { return $this->val; }
}
$b = new Builder();
for ($i = 0; $i < 500; $i++) { $b->inc()->dbl(); }
echo $b->result() . "\n";'

run_workload "3. multi-object service chain x1000" '<?php
class Adder { public function apply($x) { return $x + 1; } }
class Doubler { public function apply($x) { return $x * 2; } }
$adder = new Adder(); $doubler = new Doubler();
$r = 0;
for ($i = 0; $i < 1000; $i++) { $r = $doubler->apply($adder->apply($r)); }
echo $r . "\n";'

run_workload "4. method recursion — fib(20) via \$this->fib()" '<?php
class MathHelper {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$m = new MathHelper();
echo $m->fib(20) . "\n";'

run_workload "5. static method calls x1000" '<?php
class Calculator {
    public static function add($a, $b) { return $a + $b; }
    public static function mul($a, $b) { return $a * $b; }
}
$r = 0;
for ($i = 0; $i < 1000; $i++) { $r = Calculator::add($r, Calculator::mul($i, 2)); }
echo $r . "\n";'

run_workload "6. mixed object+scalar — Stats::record x1000" '<?php
class Stats {
    public $count = 0; public $sum = 0;
    public function record($v) { $this->count = $this->count + 1; $this->sum = $this->sum + $v; }
    public function avg() { if ($this->count == 0) return 0; return $this->sum; }
}
$st = new Stats();
for ($i = 0; $i < 1000; $i++) { $st->record($i * 2 + 1); }
echo $st->avg() . "\n";'

run_workload "7. simple method — no property, pure compute x1000" '<?php
class Calc {
    public function compute($a, $b) { return $a * 2 + $b; }
}
$c = new Calc();
$r = 0;
for ($i = 0; $i < 1000; $i++) { $r = $c->compute($r, $i); }
echo $r . "\n";'

run_workload "8. deep method recursion — ack(3,4) via method" '<?php
class Acker {
    public function ack($m, $n) {
        if ($m == 0) return $n + 1;
        if ($n == 0) return $this->ack($m - 1, 1);
        return $this->ack($m - 1, $this->ack($m, $n - 1));
    }
}
$a = new Acker();
echo $a->ack(3, 4) . "\n";'
