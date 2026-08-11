<?php

class GenericMethodTurbofishParent
{
    public function step<T>(T $value): T
    {
        return $value + 1;
    }
}

class GenericMethodTurbofishChild extends GenericMethodTurbofishParent
{
}

$box = new GenericMethodTurbofishChild();
$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = $box->step::<int>($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
