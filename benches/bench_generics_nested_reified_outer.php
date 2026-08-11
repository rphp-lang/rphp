<?php

class NestedReifiedOuterValue<T>
{
}

function nestedReifiedOuter<T>(T $value): T
{
    return $value;
}

$value = new NestedReifiedOuterValue::<int>();
$start = microtime(true);
for ($i = 0; $i < 2000000; $i++) {
    $result = nestedReifiedOuter::<NestedReifiedOuterValue>($value);
}
$elapsed = microtime(true) - $start;

echo ($result instanceof NestedReifiedOuterValue ? 'ok' : 'bad') . '|' . $elapsed;
