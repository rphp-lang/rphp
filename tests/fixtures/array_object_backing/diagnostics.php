<?php
$view = new ArrayObject(['stable'=>41]);
foreach ([null, false, 7, 'not-array'] as $value) {
    try { $view->exchangeArray($value); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
    echo $view['stable'], "\n";
}
class BackingNotice { public $replacement = 43; }
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; throw new Exception('diagnostic interrupt'); });
try { $view->exchangeArray(new BackingNotice); }
catch (Exception $error) { echo $error->getMessage(), "\n"; }
restore_error_handler();
echo $view['stable'], "\n";
$view->exchangeArray(array: ['named'=>47]);
echo $view['named'], "\n";
