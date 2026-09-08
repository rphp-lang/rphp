<?php
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
$number = 6;
$record = (object)['score'=>&$number, 'other'=>17];
$view = new ArrayObject($record);
$before = (array)$record;
$alias =& $view['score'];
$alias = 19;
echo $number, ':', $record->score, ':', $before['score'], "\n";
$view['other'] = 23;
echo $before['other'], ':', $record->other, "\n";
$copy = $view->getArrayCopy();
$copy['score'] = 29;
echo $alias, ':', $record->score, "\n";
$view->exchangeArray(['score'=>31]);
$alias = 37;
echo $number, ':', $view['score'], "\n";
