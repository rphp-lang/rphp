<?php
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
$root = new ArrayObject(['item'=>5]);
$layer = new ArrayObject($root);
$cursor = $layer->getIterator();
$cursor['item'] = 11;
echo get_class($cursor), ':', $root['item'], ':', $layer['item'], "\n";
$root->exchangeArray(['next'=>18]);
echo $cursor['next'], ':', count($cursor), "\n";
$layer->exchangeArray(['last'=>27]);
echo $cursor['last'], ':', $root['next'], "\n";
foreach ($layer as $key=>$value) echo $key, '=', $value, "\n";
$bytes = new ArrayObject(["\0raw"=>'safe']);
$wrapped = new ArrayObject(new ArrayIterator($bytes));
foreach ($wrapped as $key=>$value) echo bin2hex($key), ':', $value, "\n";
