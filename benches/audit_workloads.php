<?php
// Coverage audit workloads for hot tier
// Each section is self-contained, prints a label + result

// ── 1. fib (scalar recursion) ──
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
echo "fib(25)=" . fib(25) . "\n";

// ── 2. gcd (euclidean, loop-called) ──
function gcd($a, $b) {
    if ($b == 0) return $a;
    return gcd($b, $a % $b);
}
$sum = 0;
for ($i = 1; $i <= 100; $i++) { $sum = $sum + gcd(120, $i); }
echo "gcd_sum=" . $sum . "\n";

// ── 3. ackermann (deep multi-branch recursion) ──
function ack($m, $n) {
    if ($m == 0) return $n + 1;
    if ($n == 0) return ack($m - 1, 1);
    return ack($m - 1, ack($m, $n - 1));
}
echo "ack(3,5)=" . ack(3, 5) . "\n";

// ── 4. power (simple recursion) ──
function power($base, $exp) {
    if ($exp <= 0) return 1;
    return $base * power($base, $exp - 1);
}
echo "pow(2,20)=" . power(2, 20) . "\n";

// ── 5. function call chain (non-recursive, many calls) ──
function step1($x) { return $x + 1; }
function step2($x) { return step1($x) + 1; }
function step3($x) { return step2($x) + 1; }
$r = 0;
for ($i = 0; $i < 1000; $i++) { $r = step3($r); }
echo "chain=" . $r . "\n";

// ── 6. method calls ──
class Counter {
    public $val = 0;
    public function inc() { $this->val = $this->val + 1; return $this; }
    public function get() { return $this->val; }
}
$c = new Counter();
for ($i = 0; $i < 1000; $i++) { $c->inc(); }
echo "method=" . $c->get() . "\n";

// ── 7. closure calls ──
$add = function($a, $b) { return $a + $b; };
$r = 0;
for ($i = 0; $i < 1000; $i++) { $r = $add($r, $i); }
echo "closure=" . $r . "\n";

// ── 8. typed function (should stay cold) ──
function typed_mul(int $a, int $b): int {
    return $a * $b;
}
$r = 1;
for ($i = 1; $i <= 20; $i++) { $r = typed_mul($r, 1); }
echo "typed=" . $r . "\n";

// ── 10. global var function (should stay cold) ──
$g_count = 0;
function g_inc() {
    global $g_count;
    $g_count = $g_count + 1;
}
for ($i = 0; $i < 100; $i++) { g_inc(); }
echo "global=" . $g_count . "\n";

// ── 11. mutual recursion ──
function is_even($n) {
    if ($n == 0) return 1;
    return is_odd($n - 1);
}
function is_odd($n) {
    if ($n == 0) return 0;
    return is_even($n - 1);
}
echo "mutual=" . is_even(100) . is_odd(100) . "\n";

// ── 12. app-like: nested scalar dispatch ──
function process_item($type, $val) {
    if ($type == 1) return $val * 2;
    if ($type == 2) return $val + 10;
    return $val;
}
function batch_process($n) {
    $total = 0;
    for ($i = 0; $i < $n; $i++) {
        $total = $total + process_item($i - intval($i / 3) * 3, $i);
    }
    return $total;
}
echo "app=" . batch_process(1000) . "\n";
