<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
symlink('new-target', 'dangling');
var_dump(touch('dangling', 123, 456));
echo filemtime('new-target'), ':', fileatime('new-target'), ':', readlink('dangling'), "\n";
symlink('dangling', 'chain');
var_dump(touch('chain', 789, 321));
echo filemtime('new-target'), ':', fileatime('new-target'), ':', readlink('chain'), "\n";
link('new-target', 'hard');
var_dump(touch('hard', 111, 222));
echo filemtime('new-target'), ':', fileatime('new-target'), "\n";
