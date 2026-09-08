<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
file_put_contents('source', 'kept');
echo stat('source')['nlink'], "\n";
var_dump(link('source', 'hard'));
echo stat('source')['nlink'], "\n";
clearstatcache();
echo stat('source')['nlink'], "\n";
var_dump(symlink('source', 'soft'));
echo filetype('soft'), ':', lstat('soft')['nlink'], "\n";
var_dump(link('source', 'harder'));
echo stat('source')['nlink'], "\n";
unlink('hard');
echo stat('source')['nlink'], "\n";
