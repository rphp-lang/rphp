--TEST--
INI entries normalize spacing and expand the php-src PWD placeholder
--INI--
auto_prepend_file  = {PWD}/ini-prepend.inc
--FILE--
<?php echo 'body'; ?>
--EXPECT--
prelude-body
