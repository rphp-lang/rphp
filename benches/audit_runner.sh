#!/bin/bash
# Run each audit workload in isolation to get per-workload bail stats
set -e

RPHP="./target/debug/rphp"

echo "=== Coverage Audit ==="
echo ""

run_workload() {
    local name="$1"
    local code="$2"
    echo "── $name ──"
    echo "$code" | $RPHP 2>&1
    echo ""
}

run_workload "1. fib(25) — scalar recursion" '<?php
function fib($n) { if ($n <= 1) return $n; return fib($n-1) + fib($n-2); }
echo fib(25) . "\n";'

run_workload "2. gcd — euclidean recursion (loop)" '<?php
function gcd($a, $b) { if ($b == 0) return $a; $q = intval($a / $b); return gcd($b, $a - $q * $b); }
$s = 0; for ($i = 1; $i <= 100; $i++) { $s = $s + gcd(120, $i); } echo $s . "\n";'

run_workload "3. ackermann(3,5)" '<?php
function ack($m, $n) { if ($m == 0) return $n + 1; if ($n == 0) return ack($m-1, 1); return ack($m-1, ack($m, $n-1)); }
echo ack(3, 5) . "\n";'

run_workload "4. power(2,20) — simple recursion" '<?php
function power($b, $e) { if ($e <= 0) return 1; return $b * power($b, $e - 1); }
echo power(2, 20) . "\n";'

run_workload "5. call chain — step1→step2→step3 x1000" '<?php
function step1($x) { return $x + 1; }
function step2($x) { return step1($x) + 1; }
function step3($x) { return step2($x) + 1; }
$r = 0; for ($i = 0; $i < 1000; $i++) { $r = step3($r); } echo $r . "\n";'

run_workload "6. method calls — Counter::inc x1000" '<?php
class Counter { public $val = 0; public function inc() { $this->val = $this->val + 1; return $this; } public function get() { return $this->val; } }
$c = new Counter(); for ($i = 0; $i < 1000; $i++) { $c->inc(); } echo $c->get() . "\n";'

run_workload "7. closure calls x1000" '<?php
$add = function($a, $b) { return $a + $b; };
$r = 0; for ($i = 0; $i < 1000; $i++) { $r = $add($r, $i); } echo $r . "\n";'

run_workload "8. typed function — int hints (should stay cold)" '<?php
function typed_add(int $a, int $b): int { return $a + $b; }
$s = 0; for ($i = 0; $i < 100; $i++) { $s = typed_add($s, $i); } echo $s . "\n";'

run_workload "9. global var function (should stay cold)" '<?php
$g = 0; function g_inc() { global $g; $g = $g + 1; }
for ($i = 0; $i < 100; $i++) { g_inc(); } echo $g . "\n";'

run_workload "10. mutual recursion — is_even/is_odd" '<?php
function is_even($n) { if ($n == 0) return 1; return is_odd($n - 1); }
function is_odd($n) { if ($n == 0) return 0; return is_even($n - 1); }
echo is_even(100) . is_odd(100) . "\n";'

run_workload "11. app-like — process_item dispatch x1000" '<?php
function process_item($t, $v) { if ($t == 1) return $v * 2; if ($t == 2) return $v + 10; return $v; }
$total = 0; for ($i = 0; $i < 1000; $i++) { $total = $total + process_item($i - intval($i / 3) * 3, $i); } echo $total . "\n";'

run_workload "12. recursive with conditional string return" '<?php
function maybe_str($n) { if ($n <= 0) return "done"; return maybe_str($n - 1); }
echo maybe_str(20) . "\n";'
