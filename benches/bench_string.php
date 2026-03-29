<?php
// String concatenation: 200K appends
$t = microtime(true);
$s = '';
for ($i = 0; $i < 200000; $i++) {
    $s .= 'x';
}
$elapsed = microtime(true) - $t;
echo strlen($s) . '|' . $elapsed;
