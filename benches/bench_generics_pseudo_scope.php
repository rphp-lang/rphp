<?php

trait GenericScopedTrait<T>
{
    public self<T> $peer;

    public function same(self<T> $value): self<T>
    {
        return $value;
    }
}

class GenericScopedBase
{
    use GenericScopedTrait<int>;
}

class GenericScopedChild extends GenericScopedBase
{
}

$receiver = new GenericScopedChild();
$value = new GenericScopedBase();
$start = microtime(true);
for ($i = 0; $i < 2000000; $i++) {
    $receiver->peer = $value;
    $value = $receiver->same($value);
}
$elapsed = microtime(true) - $start;

echo ($value instanceof GenericScopedBase ? 'ok' : 'bad') . '|' . $elapsed;
