<?php
// Original AMD64 specimens: retain exact floating bits, including signed zero.
$values = [0.0, -0.0, 1e-320, -1e-320, 1e-16, -1e-16, 0.5, -0.5,
    1.0, -1.0, 1.0000000000000002, 0.9999999999999999, 2.0, -2.0,
    32.0, -32.0, 1e308, -1e308, 710.0, -750.0, PHP_INT_MAX, PHP_INT_MIN,
    INF, -INF, NAN];
foreach (['acosh', 'asinh', 'atanh', 'expm1', 'log1p'] as $name) {
    foreach ($values as $index => $value) {
        $result = $name($value);
        echo $name, ':', $index, '|', serialize($result), '|', bin2hex(pack('d', $result)), "\n";
    }
}
// Nearby already-present operations are not changed by the new registrations.
foreach (['sinh', 'cosh', 'tanh', 'exp', 'log'] as $name) {
    $result = $name(0.5);
    echo 'adjacent:', $name, '|', serialize($result), '|', bin2hex(pack('d', $result)), "\n";
}
