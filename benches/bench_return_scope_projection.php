<?php
class ReturnScopePayload {}
class ReturnScopeConsumer {
    public function arrayValue($value): array { return $value; }
    public function namedValue($value): ReturnScopePayload { return $value; }
    public function relativeValue($value): self|int { return $value; }
}
$consumer = new ReturnScopeConsumer;
$payload = new ReturnScopePayload;
$items = [1, 2, 3];
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 100000; $i++) {
    $copy = $consumer->arrayValue($items);
    $sum += count($copy);
    $sum += (int) ($consumer->namedValue($payload) === $payload);
    $sum += $consumer->relativeValue(3);
}
echo $sum, '|', microtime(true) - $start, "\n";
