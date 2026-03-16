<?php
gc_disable();
function fib($n) {
    if ($n <= 1) { return $n; }
    return fib($n - 1) + fib($n - 2);
}
$result = fib(30);
echo $result;
