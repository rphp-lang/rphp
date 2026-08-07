<?php

function callback_pipeline_repeated_map($value)
{
    return $value * 3 + 1;
}

function callback_pipeline_repeated_keep($value)
{
    return $value & 1;
}

function callback_pipeline_repeated_sum($carry, $value)
{
    return $carry + $value;
}

function callback_pipeline_repeated_run($values)
{
    return array_reduce(
        array_filter(
            array_map("callback_pipeline_repeated_map", $values),
            "callback_pipeline_repeated_keep"
        ),
        "callback_pipeline_repeated_sum",
        0
    );
}

$values = [0, 1, 2, 3, 4, 5];
$iterations = 100000;
$checksum = 0;

$startedAt = microtime(true);
for ($index = 0; $index < $iterations; $index++) {
    $checksum += callback_pipeline_repeated_run($values);
}
$elapsed = microtime(true) - $startedAt;

echo $checksum . "|" . $elapsed;
