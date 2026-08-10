<?php

function genericStep<T : int>(T $value): T
{
    return $value + 1;
}

$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = genericStep::<int>($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
