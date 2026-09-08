<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
file_put_contents('source', 'initial');
var_dump(link('source', 'hard'), symlink('source', 'soft'));
var_dump(fileinode('source') === fileinode('hard'), filetype('hard'), filetype('soft'));
echo readlink('soft'), ':', file_get_contents('soft'), "\n";
file_put_contents('hard', 'updated');
echo file_get_contents('source'), ':', file_get_contents('soft'), "\n";
unlink('source');
var_dump(file_exists('soft'), is_link('soft'), file_exists('hard'));
echo readlink('soft'), ':', file_get_contents('hard'), "\n";
var_dump(rename('soft', 'moved'), readlink('moved'));
var_dump(link('hard', 'second'), fileinode('hard') === fileinode('second'));
