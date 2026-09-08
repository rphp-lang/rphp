<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
touch('first', 100, 200);
echo stat('first')['mtime'], "\n";
var_dump(touch('first', 300, 400));
echo stat('first')['mtime'], ':', stat('first')['atime'], "\n";
symlink('first', 'soft');
$before = lstat('soft')['mtime'];
touch('soft', 500, 600);
echo stat('first')['mtime'], ':', (int)(lstat('soft')['mtime'] === $before), "\n";
set_error_handler(function () {});
var_dump(touch('missing/child', 123));
echo stat('first')['mtime'], "\n";
