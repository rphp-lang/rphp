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
$mapped = array_map("callback_pipeline_map", $values);
$filtered = array_filter($mapped, "callback_pipeline_keep");
$sum = array_reduce($filtered, "callback_pipeline_sum", 0);
$elapsed = microtime(true) - $startedAt;

echo $sum . ":" . count($filtered) . "|" . $elapsed;
