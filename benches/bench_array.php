<?php
$a = [];
for ($i = 0; $i < 50000; $i++) {
    $a[] = $i;
}
$sum = 0;
for ($i = 0; $i < 50000; $i++) {
    $sum += $a[$i];
}
echo $sum;
