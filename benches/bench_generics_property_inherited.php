<?php

class GenericPropertyParent<T>
{
    public T $value;
}

class GenericPropertyChild<U> extends GenericPropertyParent<U>
{
}

$box = new GenericPropertyChild::<int>();
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
echo $box->value;
