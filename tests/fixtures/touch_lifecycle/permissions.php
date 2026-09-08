<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
file_put_contents('owned', 'preserved');
chmod('owned', 0100);
var_dump(touch('owned', 1700000000, 1600000000));
clearstatcache();
echo filemtime('owned'), ':', fileatime('owned'), ':', fileperms('owned') & 0777, "\n";
chmod('owned', 0600);
echo file_get_contents('owned'), "\n";
