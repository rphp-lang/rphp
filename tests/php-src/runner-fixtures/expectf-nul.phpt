--TEST--
EXPECTF supports the php-src NUL-byte placeholder
--FILE--
<?php echo "before", chr(0), "after"; ?>
--EXPECTF--
before%0after
