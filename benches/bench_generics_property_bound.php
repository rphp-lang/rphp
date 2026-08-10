<?php

class BoundPropertyBox<T : int>
{
    public T $value;
}

$box = new BoundPropertyBox::<int>();
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
echo $box->value;
