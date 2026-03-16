<?php
gc_disable();
$s = '';
for ($i = 0; $i < 10000; $i++) {
    $s .= 'x';
}
echo strlen($s);
