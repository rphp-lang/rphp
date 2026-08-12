--TEST--
SKIPIF keeps skipped cases outside pass and fail
--SKIPIF--
<?php die('skip fixture precondition'); ?>
--FILE--
<?php echo 'must not run'; ?>
--EXPECT--
must not run
