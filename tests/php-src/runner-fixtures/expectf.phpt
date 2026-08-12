--TEST--
EXPECTF supports scalar, wildcard and raw-regex placeholders
--FILE--
<?php echo "value=42 path=/tmp/item id=abc123\n"; ?>
--EXPECTF--
value=%d path=%s id=%r[a-z]+\d+%r
