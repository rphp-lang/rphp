<?php
class Calculator {
    public function fib($n) {
        if ($n <= 1) { return $n; }
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$c = new Calculator();
$result = $c->fib(30);
echo $result;
