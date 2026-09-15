<?php
$input = '0.5';
$alias =& $input;
$copy = $input;
$items = [$input];
$itemsCopy = $items;
foreach (['acosh', 'asinh', 'atanh', 'expm1', 'log1p'] as $name) {
    echo $name, ':', serialize($name($alias)), ':', serialize($name($items[0])), "\n";
}
echo serialize([$input, $alias, $copy, $items, $itemsCopy]), "\n";
set_error_handler(function($level, $message) {
    echo 'handler:', $level, ':', $message, ':nested:', serialize(expm1(0.0)), "\n";
    throw new Exception('conversion interrupted');
});
foreach (['acosh', 'asinh', 'atanh', 'expm1', 'log1p'] as $name) {
    $output = 'untouched';
    try { $output = $name(null); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
    echo $name, ':after:', $output, ':', serialize($name(0.5)), "\n";
}
restore_error_handler();
function floatArgument($label, $value) { echo 'argument:', $label, "\n"; return $value; }
try { log1p(floatArgument('first', []), floatArgument('surplus', 1)); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
echo 'final:', serialize([$input, $items, $itemsCopy]), "\n";
