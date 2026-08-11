<?php

class NestedReifiedValue<T>
{
}

function nestedReified<T>(T $value): T
{
    return $value;
}

$value = new NestedReifiedValue::<int>();
$start = microtime(true);
for ($i = 0; $i < 2000000; $i++) {
    $result = nestedReified::<NestedReifiedValue<int>>($value);
}
$elapsed = microtime(true) - $start;

echo ($result instanceof NestedReifiedValue ? 'ok' : 'bad') . '|' . $elapsed;
