<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
mkdir('nested');
file_put_contents('source', 'payload');
var_dump(symlink('../source', 'nested/first'));
var_dump(symlink('first', 'nested/second'));
echo readlink('nested/first'), ':', readlink('nested/second'), ':', file_get_contents('nested/second'), "\n";
var_dump(symlink('absent', 'dangling'), symlink('loop', 'loop'));
var_dump(is_link('dangling'), file_exists('dangling'), is_link('loop'), file_exists('loop'));
echo readlink('dangling'), ':', readlink('loop'), "\n";
var_dump(link('nested/first', 'hard-symbol'));
var_dump(is_link('hard-symbol'));
echo readlink('hard-symbol'), "\n";
