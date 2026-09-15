<?php
set_error_handler(function($level, $message) { echo 'diagnostic:', $level, ':', $message, "\n"; return true; });
function observeFloat($label, $call) {
    echo $label, "\n";
    try { echo serialize($call()), "\n"; }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
class FloatText {
    public function __toString() { echo "unexpected string conversion\n"; return '1.5'; }
}
$resource = fopen('php://memory', 'r+');
foreach (['acosh', 'asinh', 'atanh', 'expm1', 'log1p'] as $name) {
    observeFloat($name . ':zero', fn() => $name());
    observeFloat($name . ':extra', fn() => $name(0.5, 2));
    observeFloat($name . ':bad-name', fn() => $name(value: 0.5));
    observeFloat($name . ':named', fn() => $name(num: 0.5));
    observeFloat($name . ':unpack', fn() => $name(...['num' => 0.5]));
    observeFloat($name . ':callback', fn() => call_user_func($name, 0.5));
    observeFloat($name . ':first-class', fn() => ($name(...))(0.5));
    foreach ([null, true, false, 1, -1, '1.5', " \t1e-16\n", '1e999', '', '1x', [],
        new stdClass, new FloatText, $resource] as $index => $value) {
        observeFloat($name . ':weak:' . $index, fn() => $name($value));
    }
    foreach (['1', '0.5', '"1.5"', 'true', 'null', 'new FloatText'] as $index => $expression) {
        observeFloat($name . ':strict:' . $index,
            fn() => eval('declare(strict_types=1); return ' . $name . '(' . $expression . ');'));
    }
    observeFloat($name . ':signature', function() use ($name) {
        $function = new ReflectionFunction($name);
        $parameter = $function->getParameters()[0];
        return [$function->getName(), $function->getNumberOfParameters(), $function->getNumberOfRequiredParameters(),
            $parameter->getName(), (string)$parameter->getType(), $parameter->isPassedByReference(),
            $parameter->isOptional(), (string)$function->getReturnType()];
    });
}
fclose($resource);
observeFloat('closed-resource', fn() => asinh($resource));
