<?php
function &lifetime() {
    $cell = ['kept'];
    yield $cell;
}
$stream = lifetime();
foreach ($stream as &$cell) { break; }
unset($stream);
echo "detached\n";
$cell[] = 'updated';
echo json_encode($cell), "\n";
unset($cell);
echo "done\n";
function &snapshotCell(&$cell) { yield $cell; $cell = 33; yield $cell; }
$cell = 12;
$stream = snapshotCell($cell);
$first = $stream->current();
$cell = 21;
echo $first, ':', $stream->current(), "\n";
$values = iterator_to_array($stream);
$cell = 44;
echo json_encode($values), "\n";
