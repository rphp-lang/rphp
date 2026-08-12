--TEST--
CLEAN executes after a passing test
--FILE--
<?php
file_put_contents(__DIR__ . '/clean.tmp', 'ok');
echo file_get_contents(__DIR__ . '/clean.tmp');
?>
--CLEAN--
<?php
unlink(__DIR__ . '/clean.tmp');
?>
--EXPECT--
ok
