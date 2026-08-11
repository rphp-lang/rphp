<?php

trait ManualScopedTrait
{
    public self $peer;

    public function same(self $value): self
    {
        return $value;
    }
}

class ManualScopedBase
{
    use ManualScopedTrait;
}

class ManualScopedChild extends ManualScopedBase
{
}

$receiver = new ManualScopedChild();
$value = new ManualScopedBase();
$start = microtime(true);
for ($i = 0; $i < 2000000; $i++) {
    $receiver->peer = $value;
    $value = $receiver->same($value);
}
$elapsed = microtime(true) - $start;

echo ($value instanceof ManualScopedBase ? 'ok' : 'bad') . '|' . $elapsed;
