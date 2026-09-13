<?php
function ordinaryCatchReturn($value) {
    try { return $value + 1; } catch (Exception $error) { return -1; }
}
function replacesOwnException() {
    try { throw new Exception('replaced'); }
    finally { return ordinaryCatchReturn(8); }
}
echo ordinaryCatchReturn(1), "\n";
try {
    try { throw new Exception('outer'); }
    finally {
        echo ordinaryCatchReturn(2), "\n";
        echo ordinaryCatchReturn(3), "\n";
    }
} catch (Exception $error) { echo $error->getMessage(), "\n"; }
echo replacesOwnException(), "\n";
echo ordinaryCatchReturn(4), "\n";
