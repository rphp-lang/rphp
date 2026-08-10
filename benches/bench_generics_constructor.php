<?php

class GenericConstructorBox<T>
{
    public T $value;

    public function __construct(T $value)
    {
        $this->value = $value;
    }
}

$start = microtime(true);
$box = null;
for ($i = 0; $i < 1000000; $i++) {
    $box = new GenericConstructorBox::<int>($i);
}
$elapsed = microtime(true) - $start;

echo $box->value . '|' . $elapsed;
