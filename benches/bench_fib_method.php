<?php
// Method fib(35) — via $this->fib(), measures InitMethodCall
class Calculator {
    public function fib($n) {
        if ($n <= 1) { return $n; }
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$c = new Calculator();
$t = microtime(true);
$r = $c->fib(35);
$elapsed = microtime(true) - $t;
echo $r . '|' . $elapsed;
