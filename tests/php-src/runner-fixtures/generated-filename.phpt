--TEST--
Generated FILE section uses php-src's canonical .php basename
--FILE--
<?php echo basename(__FILE__); ?>
--EXPECT--
generated-filename.php
