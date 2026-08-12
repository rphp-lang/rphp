--TEST--
FILE retains the newline before the next PHPT section
--FILE--
<?php echo <<<END
--EXPECTF--
Parse error: syntax error, unexpected end of file in %s on line %d
