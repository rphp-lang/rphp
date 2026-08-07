<?php

function callback_pipeline_map($value)
{
    return $value * 3 + 1;
}

function callback_pipeline_keep($value)
{
    return $value & 1;
}

function callback_pipeline_sum($carry, $value)
{
    return $carry + $value;
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$sum = array_reduce(
    array_filter(
        array_map("callback_pipeline_map", $values),
        "callback_pipeline_keep"
    ),
    "callback_pipeline_sum",
    0
);
$elapsed = microtime(true) - $startedAt;

echo $sum . "|" . $elapsed;
