<?php
$path = 'data-adjacent:missing-native-open-probe';
if (file_exists($path)) { throw new Exception('Specimen requires an absent input'); }
set_error_handler(function ($level, $message) use (&$path) {
    echo $level, ':', $message, "\n";
    $path = 'data:text/plain,changed';
    return true;
});
var_dump(file_get_contents($path), $path);
restore_error_handler();
set_error_handler(function ($level, $message) {
    throw new Exception($message);
});
try {
    file_get_contents('plain-missing-native-open-probe');
    echo "unexpected continuation\n";
} catch (Exception $error) {
    echo 'caught:', $error->getMessage(), "\n";
}
restore_error_handler();
var_dump(file_get_contents('data:text/plain,unchanged'));
var_dump(strlen(file_get_contents(__FILE__)) > 0);
