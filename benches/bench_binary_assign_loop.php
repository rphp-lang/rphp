<?php
// Linear typed body: modulo plus checked add, multiply and subtraction.
$n = 10000000;
$last = 0;
$product = 0;
$remaining = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $last = 20 + ($i % 400);
    $product = $i * 73;
    $remaining = $n - $i;
}
$elapsed = microtime(true) - $t;
echo $last . ',' . $product . ',' . $remaining . '|' . $elapsed;
