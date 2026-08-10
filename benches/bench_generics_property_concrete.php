<?php

class ConcretePropertyParent<T>
{
    public T $value;
}

class ConcreteIntPropertyBox extends ConcretePropertyParent<int>
{
}

$box = new ConcreteIntPropertyBox();
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
echo $box->value;
