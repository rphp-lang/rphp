<?php
set_error_handler(function ($level, $message) { echo 'notice:', $message, "\n"; });
function &emptyCell() { yield; }
foreach (emptyCell() as &$cell) { var_dump($cell); $cell = 'retained'; }
echo $cell, "\n";
function &assignedCell(&$source) {
    yield ($local =& $source);
    echo 'local:', $local, "\n";
    $slots = [];
    yield ($slots[] =& $source);
    echo json_encode($slots), "\n";
}
$source = 3;
foreach (assignedCell($source) as &$cell) { $cell += 5; }
echo 'source:', $source, "\n";
function &reusedCell() {
    $local = 5;
    yield $local;
    foreach ([9, 10] as $local) {}
    echo 'reused:', $local, "\n";
    yield $local;
}
$aliases = [];
foreach (reusedCell() as &$cell) { $aliases[] =& $cell; }
echo json_encode($aliases), "\n";
class ReadOnlySource { public function read() { return 4; } }
function &optionalCall($source) { yield $source?->read(); }
foreach (optionalCall(new ReadOnlySource) as &$cell) { echo 'call:', $cell, "\n"; }
function &outerScope() {
    $callback = function () { if (false) { yield from []; } yield 4; };
    yield $callback;
}
foreach (outerScope() as $callback) {
    foreach ($callback() as $value) { echo 'nested:', $value, "\n"; }
}
function &failedPublication() {
    try { yield 2; }
    catch (Exception $error) { echo "inner catch\n"; }
    finally { echo "inner finally\n"; }
}
set_error_handler(function () { throw new Exception('publication'); });
$stream = failedPublication();
try { foreach ($stream as &$cell) { echo "body\n"; } }
catch (Exception $error) { echo 'outer:', $error->getMessage(), "\n"; }
var_dump($stream->valid());
