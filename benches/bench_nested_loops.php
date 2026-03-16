<?php
$sum = 0;
for ($i = 0; $i < 500; $i++) {
    for ($j = 0; $j < 500; $j++) {
        $sum += $i + $j;
    }
}
echo $sum;
