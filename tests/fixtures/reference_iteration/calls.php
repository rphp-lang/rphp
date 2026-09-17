<?php
set_error_handler(function ($level, $message) { echo 'notice:', $message, "\n"; });
function &identityCell(&$cell) { return $cell; }
function detachedValue() { echo "call\n"; return 27; }
function &produce(&$cell) {
    yield identityCell($cell);
    yield detachedValue();
    yield $temporary = 30;
    yield 41;
}
$cell = 13;
foreach (produce($cell) as &$value) { echo 'value:', $value, "\n"; $value++; }
echo 'source:', $cell, "\n";
function &suppressed() { yield 3; }
set_error_handler(function ($level, $message) { throw new Exception('blocked yield'); });
$stream = suppressed();
try { $stream->current(); } catch (Exception $error) { echo $error->getMessage(), "\n"; }
var_dump($stream->valid());
