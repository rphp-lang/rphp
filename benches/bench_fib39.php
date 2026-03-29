<?php
// Fibonacci(39) — ~200M recursive calls
function fib($n) {
    if ($n <= 1) { return $n; }
    return fib($n - 1) + fib($n - 2);
}
$t = microtime(true);
$r = fib(39);
$elapsed = microtime(true) - $t;
echo $r . '|' . $elapsed;
