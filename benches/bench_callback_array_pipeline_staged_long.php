<?php

function callback_pipeline_staged_map($value)
{
    return $value * 3 + 1;
}

function callback_pipeline_staged_keep($value)
{
    return $value & 1;
}

function callback_pipeline_staged_sum($carry, $value)
{
    return $carry + $value;
}

function callback_pipeline_staged_run($values)
{
    $mapped = array_map("callback_pipeline_staged_map", $values);
    $filtered = array_filter($mapped, "callback_pipeline_staged_keep");
    return array_reduce($filtered, "callback_pipeline_staged_sum", 0);
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$sum = callback_pipeline_staged_run($values);
$elapsed = microtime(true) - $startedAt;

echo $sum . "|" . $elapsed;
