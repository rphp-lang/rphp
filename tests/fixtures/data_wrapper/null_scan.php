<?php
$warnings = 0;
$checks = 0;
set_error_handler(function () use (&$warnings) { ++$warnings; });
foreach ([0, 1, 7, 8, 15, 16, 31, 32, 63, 64, 127, 128, 4095, 4096, 8191] as $offset) {
    // Invalid header deliberately competes with a NUL at scan boundaries.
    $path = 'data:invalid-header,' . str_repeat('q', $offset) . "\0" . 'tail';
    foreach (['file_get_contents', 'fopen', 'file'] as $function) {
        try {
            if ($function === 'fopen') { $function($path, 'r'); }
            else { $function($path); }
        } catch (ValueError $error) {
            if ($error->getMessage() !== $function . '(): Argument #1 ($filename) must not contain any null bytes') {
                throw new Exception('wrong NUL priority');
            }
            ++$checks;
        }
    }
}
// Exercise every NUL position in short paths, including overlapping word
// loads and the transition to the long-path scan. All are rejected before I/O.
for ($length = 1; $length <= 40; ++$length) {
    for ($position = 0; $position < $length; ++$position) {
        $path = str_repeat('q', $length);
        $path[$position] = "\0";
        foreach (['file_get_contents', 'fopen', 'file'] as $function) {
            try {
                if ($function === 'fopen') { $function($path, 'r'); }
                else { $function($path); }
            } catch (ValueError $error) {
                if ($error->getMessage() !== $function . '(): Argument #1 ($filename) must not contain any null bytes') {
                    throw new Exception('wrong short NUL priority');
                }
                ++$checks;
            }
        }
    }
}
restore_error_handler();
echo 'checks:', $checks, '|warnings:', $warnings, "\n";
