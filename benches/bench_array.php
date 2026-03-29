<?php
// Array build + indexed read: 500K elements
$t = microtime(true);
$a = [];
for ($i = 0; $i < 500000; $i++) {
    $a[] = $i;
}
$sum = 0;
for ($i = 0; $i < 500000; $i++) {
    $sum += $a[$i];
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
