<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
var_dump(touch('file://' . getcwd() . '/native', 123, 456));
echo filemtime('native'), ':', fileatime('native'), "\n";
mkdir('localhost');
var_dump(touch('file://localhost/host', 234, 567));
echo filemtime('localhost/host'), ':', fileatime('localhost/host'), "\n";
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
var_dump(touch('file://localhost/missing/child', 123));
var_dump(touch('file://relative', 123));
var_dump(touch('http://not-a-file', 123));
var_dump(touch('timestamp-unknown://missing', 123));
