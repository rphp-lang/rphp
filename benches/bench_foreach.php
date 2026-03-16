<?php
$a = [];
for ($i = 0; $i < 50000; $i++) {
    $a[] = $i;
}
$sum = 0;
foreach ($a as $v) {
    $sum += $v;
}
echo $sum;
