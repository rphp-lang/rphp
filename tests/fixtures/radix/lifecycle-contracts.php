<?php
function failRadix($label, $call) {
    try { echo $label, ':', serialize($call()), "\n"; }
    catch (Throwable $error) { echo $label, ':', get_class($error), ':', $error->getMessage(), "\n"; }
}
$input = "10\xff01";
$alias =& $input;
$copy = $input;
set_error_handler(function($level, $message) { echo 'notice:', $level, ':', $message, "\n"; return true; });
echo bindec($alias), '|', bin2hex($input), '|', bin2hex($copy), "\n";
$number = -9;
$numberAlias =& $number;
echo decoct($numberAlias), ':', $number, ':', $numberAlias, "\n";
class NestedRadixText {
    public function __toString() { echo 'nested:', hexdec('ff'), "\n"; return '1?01'; }
}
echo 'outer:', bindec(new NestedRadixText), "\n";
class ThrowingRadixText {
    public function __toString() { echo "throwing conversion\n"; throw new Exception('conversion stopped'); }
}
failRadix('object-failure', fn() => octdec(new ThrowingRadixText));
set_error_handler(function($level, $message) { throw new Exception($level . ':' . $message); });
foreach (['bindec', 'octdec', 'hexdec'] as $name) {
    failRadix($name . ':invalid', fn() => $name('!?1!?'));
    failRadix($name . ':null', fn() => $name(null));
    failRadix($name . ':nan', fn() => $name(NAN));
}
failRadix('fraction', fn() => decoct(7.5));
restore_error_handler();
echo 'recovered:', bindec('101'), ':', octdec('17'), ':', hexdec('2f'), ':', decoct(63), "\n";
echo 'state:', bin2hex($input), ':', bin2hex($copy), ':', $number, "\n";
restore_error_handler();
