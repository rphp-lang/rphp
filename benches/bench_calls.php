<?php
// Call-heavy: 3 levels of function calls, 5M iterations
function add1($x) { return $x + 1; }
function double($x) { return $x + $x; }
function combine($a, $b) { return add1($a) + double($b); }

$t = microtime(true);
$sum = 0;
for ($i = 0; $i < 5000000; $i++) {
    $sum += combine($i, $i + 1);
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
