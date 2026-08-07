<?php

function callback_pipeline_filter_map_keep($value)
{
    return $value & 1;
}

function callback_pipeline_filter_map_map($value)
{
    return $value * 3 + 1;
}

function callback_pipeline_filter_map_sum($carry, $value)
{
    return $carry + $value;
}

function callback_pipeline_filter_map_run($values)
{
    $filtered = array_filter($values, "callback_pipeline_filter_map_keep");
    $mapped = array_map("callback_pipeline_filter_map_map", $filtered);
    return array_reduce($mapped, "callback_pipeline_filter_map_sum", 0);
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$sum = callback_pipeline_filter_map_run($values);
$elapsed = microtime(true) - $startedAt;

echo $sum . "|" . $elapsed;
