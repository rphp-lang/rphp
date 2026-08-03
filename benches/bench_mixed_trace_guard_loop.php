<?php

// Mixed typed region: monomorphic object call, borrowed string state, dynamic
// hash update, internal routing branch, and one arbitrary cold PHP edge.
class MixedTraceGuardModel
{
    public function score(int $value, string $key): int
    {
        return $value + strlen($key);
    }
}

$model = new MixedTraceGuardModel();
$values = ['left' => 0, 'right' => 0];
$key = 'left';
$needle = -1;
$n = 1000000;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
    $score = $model->score($i, $key);
    $values[$key] = $values[$key] + $score;
    if ($i === $needle) {
        echo 'never';
    }
}
$elapsed = microtime(true) - $t;
echo $values['left'] . ':' . $values['right'] . '|' . $elapsed;
