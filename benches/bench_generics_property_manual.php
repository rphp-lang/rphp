<?php

class ManualPropertyBox
{
    public $value;
}

$box = new ManualPropertyBox();
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
echo $box->value;
