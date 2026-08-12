--TEST--
EXPECTREGEX matches the complete normalized output
--FILE--
<?php echo "item-2048"; ?>
--EXPECTREGEX--
item-[0-9]{4}
