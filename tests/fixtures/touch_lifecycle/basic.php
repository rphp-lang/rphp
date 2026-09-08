<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
var_dump(touch('fresh', 1700000123, 1600000456));
echo filemtime('fresh'), ':', fileatime('fresh'), ':', filesize('fresh'), "\n";
file_put_contents('kept', 'unchanged payload');
var_dump(touch('kept', 1700000000, 1600000000));
echo filemtime('kept'), ':', fileatime('kept'), ':', file_get_contents('kept'), "\n";
mkdir('directory');
var_dump(touch('directory', 100, -100));
echo filemtime('directory'), ':', fileatime('directory'), "\n";
foreach ([-1, 0, 2147483648, 4102444800] as $stamp) {
    var_dump(touch('fresh', $stamp));
    echo filemtime('fresh'), ':', fileatime('fresh'), "\n";
}
