<?php
// Fibonacci(35) — ~29M recursive calls
function fib($n) {
    if ($n <= 1) { return $n; }
    return fib($n - 1) + fib($n - 2);
}
$t = microtime(true);
$r = fib(35);
$elapsed = microtime(true) - $t;
echo $r . '|' . $elapsed;
