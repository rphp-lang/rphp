--TEST--
STDIN is delivered byte-for-byte
--STDIN--
fixture input
--FILE--
<?php echo stream_get_contents(STDIN); ?>
--EXPECT--
fixture input
