<?php
set_error_handler(function($level, $message) {});
class ReleasedEntry {
    function __construct(public $label) {}
    function __destruct() {
        global $view;
        echo 'drop:', $this->label, ':', count($view), "\n";
        $view->exchangeArray(['after'=>53]);
    }
}
$view = new ArrayObject((object)['entry'=>new ReleasedEntry('slot')]);
unset($view['entry']);
echo $view['after'], "\n";
$view->exchangeArray(new ReleasedEntry('owner'));
$snapshot = $view->exchangeArray(['temporary'=>59]);
echo $view['after'], ':', $snapshot['label'], "\n";
