<?php

class MixedStringHashModel
{
    public function score(int $value, string $key): int
    {
        return $value + strlen($key);
    }
}

$model = new MixedStringHashModel();
$values = ['left' => 0, 'right' => 0];
$key = 'left';
$needle = -1;
$start = microtime(true);
for ($i = 0; $i < 1000000; $i++) {
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
$elapsed = microtime(true) - $start;
echo $values['left'] . ':' . $values['right'] . ':' . $i . '|' . $elapsed;
