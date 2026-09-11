<?php
function marker_route($mode, &$trace) {
    try {
        try {
            $trace .= 'a';
            if ($mode === 0) { return 7; }
            if ($mode === 1) { throw new Exception('old'); }
            if ($mode === 2) { goto after; }
            return 10;
        } finally {
            $trace .= 'b';
            try { $trace .= 'c'; } finally { $trace .= 'd'; }
            if ($mode === 3) { return 11; }
            if ($mode === 4) { throw new Exception('new'); }
        }
    } finally {
        $trace .= 'e';
    }
after:
    $trace .= 'f';
    return 12;
}
for ($mode = 0; $mode < 3; $mode++) {
    $trace = '';
    try { echo marker_route($mode, $trace); }
    catch (Exception $error) { echo $error->getMessage(); }
    finally { $trace .= 'g'; }
    echo ':', $trace, "\n";
}
$trace = '';
for ($i = 0; $i < 3; $i++) {
    try {
        if ($i === 0) { continue; }
        if ($i === 1) { break; }
    } finally { $trace .= $i; }
}
echo 'loop:', $trace, "\n";
class MarkerDestructor {
    function __destruct() { echo 'destructor;'; throw new Exception('retired'); }
}
function marker_retire() {
    $local = new MarkerDestructor;
    try { return 4; } finally { echo 'finally;'; }
}
try { marker_retire(); } catch (Exception $error) { echo $error->getMessage(), "\n"; }
function marker_generator() {
    try { yield 1; return 2; } finally { echo 'generator-finally;'; }
}
function marker_caught_inside_finally() {
    try { return 5; } finally {
        try { throw new Exception('inner'); }
        catch (Exception $error) { echo $error->getMessage(), ';'; }
    }
}
function marker_called() { try { return 'i'; } finally { echo 'called;'; } }
function marker_reentrant(&$trace) {
    try { return 6; } finally { $trace .= marker_called(); }
}
$trace = '';
echo marker_reentrant($trace), ':', $trace, "\n";
