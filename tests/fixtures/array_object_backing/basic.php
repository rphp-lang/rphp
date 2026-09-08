<?php
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
$seed = ['north'=>2];
$view = new ArrayObject($seed);
$seed['north'] = 7;
$view['west'] = 4;
echo $view['north'], ':', count($view), ':', count($seed), "\n";
$object = (object)['east'=>9];
$previous = $view->exchangeArray($object);
$previous['west'] = 10;
$view['east'] = 12;
$object->south = 15;
echo $object->east, ':', $view['south'], ':', count($view), ':', $previous['west'], "\n";
$copy = $view->getArrayCopy();
$copy['east'] = 30;
echo $view['east'], ':', $copy['east'], "\n";
$old = $view->exchangeArray(['fresh'=>21]);
echo $old['east'], ':', $old['south'], ':', $view['fresh'], "\n";
var_dump(get_object_vars($view));
