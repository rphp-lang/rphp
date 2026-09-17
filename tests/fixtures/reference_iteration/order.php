<?php
function keyProbe() { echo "key\n"; return 'chosen'; }
function &valueProbe(&$cell) { echo "value\n"; return $cell; }
function &ordered(&$cell) { yield keyProbe() => valueProbe($cell); echo 'resumed:', $cell, "\n"; }
$cell = 7;
foreach (ordered($cell) as $key => &$value) { echo $key, ':', $value, "\n"; $value = 9; }
function byValue() { yield 1; }
try { foreach (byValue() as &$value) {} } catch (Exception $error) { echo $error->getMessage(), "\n"; }
function &delegating(&$stream, &$cell) {
    yield $cell;
    try { $stream->next(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
    yield $cell;
}
$stream = delegating($stream, $cell);
foreach ($stream as &$value) { $value++; }
echo 'final:', $cell, "\n";
