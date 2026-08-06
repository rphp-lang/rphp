<?php
// Common mixed-array transition: retain a large numeric list and append one
// associative metadata field.
$values = range(0, 999999);
$t = microtime(true);
$values['sentinel'] = 1;
$elapsed = microtime(true) - $t;
echo count($values) . ':' . $values[999999] . ':' . $values['sentinel'] . '|' . $elapsed;
