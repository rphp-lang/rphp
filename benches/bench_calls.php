<?php
// Call-heavy benchmark: measures function call overhead.
// 3 levels of scalar-only helper functions, called in tight loop.

function add1($x) { return $x + 1; }
function double($x) { return $x + $x; }
function combine($a, $b) { return add1($a) + double($b); }

$sum = 0;
for ($i = 0; $i < 1000000; $i++) {
    $sum += combine($i, $i + 1);
}
echo $sum;
