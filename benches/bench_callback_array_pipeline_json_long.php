<?php

function callback_pipeline_json_map($value)
{
    return $value * 3 + 1;
}

function callback_pipeline_json_keep($value)
{
    return $value & 1;
}

function callback_pipeline_json_sum($carry, $value)
{
    return $carry + $value;
}

function callback_pipeline_json_run($values)
{
    return json_encode(array_reduce(
        array_filter(
            array_map("callback_pipeline_json_map", $values),
            "callback_pipeline_json_keep"
        ),
        "callback_pipeline_json_sum",
        0
    ));
}

$values = [0, 1, 2, 3, 4, 5];
$iterations = 100000;
$checksum = 0;

$startedAt = microtime(true);
for ($index = 0; $index < $iterations; $index++) {
    $encoded = callback_pipeline_json_run($values);
    $checksum += strlen($encoded);
}
$elapsed = microtime(true) - $startedAt;

echo $checksum . "|" . $elapsed;
