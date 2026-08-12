--TEST--
XFAIL remains visible without reducing the compatibility headline
--XFAIL--
Fixture intentionally differs from its expectation.
--FILE--
<?php echo 'known upstream behavior'; ?>
--EXPECT--
desired future behavior
