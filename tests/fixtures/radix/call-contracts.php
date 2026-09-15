<?php
set_error_handler(function($level, $message) { echo 'diag:', $level, ':', $message, "\n"; return true; });
function observeRadix($label, $call) {
    echo $label, "\n";
    try { echo serialize($call()), "\n"; }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
class RadixText {
    public function __toString() { echo "string-conversion\n"; return '101'; }
}
$open = fopen('php://memory', 'r+');
$closed = fopen('php://memory', 'r+');
fclose($closed);
foreach (['bindec' => 'binary_string', 'octdec' => 'octal_string', 'hexdec' => 'hex_string', 'decoct' => 'num'] as $name => $parameter) {
    observeRadix($name . ':zero', fn() => $name());
    observeRadix($name . ':extra', fn() => $name('101', 'unused'));
    observeRadix($name . ':bad-name', fn() => $name(wrong: '101'));
    observeRadix($name . ':named-unpack', fn() => $name(...[$parameter => '101']));
    observeRadix($name . ':callback', fn() => call_user_func($name, '101'));
    observeRadix($name . ':first-class', fn() => ($name(...))('101'));
    foreach ([null, false, true, 101, -101, 10.5, NAN, INF, [], new stdClass, new RadixText, $open, $closed] as $index => $value) {
        observeRadix($name . ':weak:' . $index, fn() => $name($value));
    }
    foreach (['101', '10.5', 'true', 'null', 'new RadixText', '"101"'] as $index => $expression) {
        observeRadix($name . ':strict:' . $index, fn() => eval('declare(strict_types=1); return ' . $name . '(' . $expression . ');'));
    }
    observeRadix($name . ':signature', function() use ($name) {
        $function = new ReflectionFunction($name);
        $parameter = $function->getParameters()[0];
        return [$function->getName(), $function->getNumberOfParameters(), $function->getNumberOfRequiredParameters(),
            $parameter->getName(), (string)$parameter->getType(), $parameter->isPassedByReference(),
            $parameter->isOptional(), (string)$function->getReturnType()];
    });
}
fclose($open);
foreach (['10.0', '10.5', '-10.5', '1e2', '0x10', '', " 17\n", '9223372036854775807', '9223372036854775808'] as $input) {
    observeRadix('decoct-string:' . bin2hex($input), fn() => decoct($input));
}
