<?php
class CleanupCallback {
    function values() { return [101]; }
    function __destruct() { echo "cleanup-callback\n"; }
}
function callback_value() { return [new CleanupCallback(), 101][1]; }
function plain_value() { return [101][0]; }
var_dump(plain_value(), callback_value());
