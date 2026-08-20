<?php

class ScopedStaticCallbackControl
{
    public static function step($value)
    {
        return $value + 1;
    }

    public function run($count)
    {
        $sum = 0;
        for ($index = 0; $index < $count; $index++) {
            $sum += call_user_func('ScopedStaticCallbackControl::step', $index);
        }
        return $sum;
    }
}

$startedAt = microtime(true);
$sum = (new ScopedStaticCallbackControl())->run(5000000);
$elapsed = microtime(true) - $startedAt;

echo $sum . '|' . $elapsed;
