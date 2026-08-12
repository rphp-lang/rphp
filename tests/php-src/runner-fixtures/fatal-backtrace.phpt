--TEST--
PHP 8.4 compile-time fatal diagnostics omit PHP 8.5 backtraces
--FILE--
<?php
switch (1) {
    default:
        break;
    default:
        break;
}
?>
--EXPECTF--
Fatal error: Switch statements may only contain one default clause in %s on line %d
