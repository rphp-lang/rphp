<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
file_put_contents('source', 'kept');
var_dump(symlink('future', 'alias'));
var_dump(symlink('source', 'alias'));
echo readlink('alias'), ':', readlink('future'), ':', file_get_contents('alias'), "\n";
var_dump(symlink('source', 'absent/../lexical'));
echo readlink('lexical'), "\n";
mkdir('directory');
var_dump(symlink('directory', 'dir-alias'), symlink('../source', 'dir-alias/child'));
echo readlink('directory/child'), ':', file_get_contents('directory/child'), "\n";
set_error_handler(function ($level, $message) { echo $message, "\n"; });
var_dump(link('source', 'missing/../native'));
var_dump(symlink('loop', 'loop'), symlink('source', 'loop'));
echo readlink('loop'), "\n";
foreach ([32, 33] as $depth) {
    mkdir('chain' . $depth);
    chdir('chain' . $depth);
    for ($index = $depth - 1; $index >= 0; --$index) symlink('part' . ($index + 1), 'part' . $index);
    clearstatcache(true);
    echo $depth, ':';
    var_dump(symlink('future', 'part0'));
    chdir('..');
}
