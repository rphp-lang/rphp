<?php
function &slots(&$values) {
    yield 'head' => $values[0];
    yield 'tail' => $values[1];
}
$values = [11, 23];
$copy = $values;
$stream = slots($values);
foreach ($stream as $key => &$cell) {
    echo $key, ':', $cell, "\n";
    $cell += 7;
}
$cell = 42;
echo json_encode([$values, $copy]), "\n";
function &localCell() {
    $local = 5;
    yield $local;
    echo 'resume:', $local, "\n";
    yield $local;
}
$stream = localCell();
foreach ($stream as &$cell) { $cell += 3; }
echo 'last:', $cell, "\n";
