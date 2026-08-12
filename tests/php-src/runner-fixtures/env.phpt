--TEST--
ENV entries reach the target process
--ENV--
RPHP_PHPT_FIXTURE=visible
--FILE--
<?php echo getenv('RPHP_PHPT_FIXTURE'); ?>
--EXPECT--
visible
