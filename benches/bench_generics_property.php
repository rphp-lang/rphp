<?php

class GenericPropertyBox<T>
{
    public T $value;
}

$box = new GenericPropertyBox::<int>();
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
echo $box->value;
