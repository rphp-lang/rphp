<?php
// Original AMD64 radix specimens. Floating results include their exact bits.
set_error_handler(function($level, $message) { echo 'diag:', $level, ':', $message, "\n"; return true; });
function showRadix($label, $value) {
    echo $label, '|', serialize($value);
    if (is_float($value)) echo '|', bin2hex(pack('d', $value));
    echo "\n";
}
$inputs = [
    '', '0', '101', '736', 'abCD', " \t101\r\n", "\v\f101\v\f",
    '0b101', '0B101', '0o73', '0O73', '0x4f', '0X4F', '0b', '0o', '0x',
    '+101', '-101', '1.01', '1_01', "1 0\t1", '1e3', '00000101',
    "10\0" . '11', "\0" . '101', "\xc2\xa0" . '101', "1\x80\xff01",
    'no digits', '!!!', '0x0x12', '0b0b11', "101\0 \t",
];
foreach (['bindec', 'octdec', 'hexdec'] as $name) {
    foreach ($inputs as $index => $input) showRadix($name . ':' . $index, $name($input));
}
foreach (['bindec' => '1', 'octdec' => '7', 'hexdec' => 'f'] as $name => $digit) {
    foreach ([15, 16, 17, 20, 21, 22, 52, 53, 54, 62, 63, 64, 65, 128, 1024, 1100] as $length) {
        showRadix($name . ':wide:' . $length, $name(str_repeat($digit, $length)));
    }
}
foreach ([0, 1, 7, 8, 9, 63, 64, 511, 512, PHP_INT_MAX, PHP_INT_MIN, -1, -8, -512] as $number) {
    showRadix('decoct:' . $number, decoct($number));
}
// Existing siblings must retain their conversion policy.
foreach (['0b101', "\v\f101\v\f", '1 0x1', "1\x801", str_repeat('1', 64)] as $input) {
    showRadix('adjacent', base_convert($input, 2, 8));
}
showRadix('decbin-negative', decbin(-9));
showRadix('dechex-negative', dechex(-9));
