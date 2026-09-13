<?php
$visits = 0;
set_error_handler(function ($level, $message) use (&$visits) {
    $visits++;
    echo "H:$visits\n";
    if (strpos($message, 'missingContinuation') === false) {
        throw new Exception('wrong warning');
    }
    throw new Exception('callback');
});
for ($i = 0; $i < 3; $i++) {
    try {
        $unused = $missingContinuation;
        echo "unreachable\n";
    } catch (Exception $error) {
        echo "C:$i\n";
        continue;
    } finally {
        echo "F:$i\n";
    }
}
restore_error_handler();
function continuationGenerator() {
    try {
        yield from [4, 5];
        yield 'tail' => 6;
    } finally {
        echo "GF\n";
    }
}
foreach (continuationGenerator() as $key => $value) echo "G:$key=$value\n";
function continuationDefault($value = 11) { echo "D:$value\n"; }
continuationDefault();
continuationDefault(12);
eval('function continuationEval() { return 13; }');
echo 'E:', continuationEval(), "\n";
$nothing = null;
echo 'N:', (int) ($nothing?->absent() === null), "\n";
function continuationStatic() { static $value = 1; echo 'S:', ++$value, "\n"; }
continuationStatic();
continuationStatic();
function continuationReturn() {
    try { echo "R:8\n"; return 9; }
    finally { echo "RF\n"; }
}
$value = continuationReturn();
echo "M:$value\n";
