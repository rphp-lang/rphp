<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
function ap_function($name) { echo "function:$name\n"; }
class ApBase {
    public static function Fetch($name) { echo "static:", static::class, ":$name\n"; }
    public static function add() { spl_autoload_register(['self', 'Fetch']); }
}
class ApChild extends ApBase {
    public static function add() { spl_autoload_register(['self', 'Fetch']); }
}
class ApObject {
    public function Fetch($name) { echo "object:$name\n"; }
    public function __invoke($name) { echo "invoke:$name\n"; }
}
function ap_describe() {
    foreach (spl_autoload_functions() as $callback) {
        if (is_array($callback)) {
            echo is_object($callback[0]) ? get_class($callback[0]) : $callback[0], '::', $callback[1], "\n";
        } elseif (is_object($callback)) { echo 'object:', get_class($callback), "\n"; }
        else { echo $callback, "\n"; }
    }
}
spl_autoload_register('AP_FUNCTION');
spl_autoload_register('ap_function', true, true);
spl_autoload_register('ApBase::fEtCh');
spl_autoload_register(['ApBase', 'FETCH']);
ApBase::add();
ApChild::add();
$object = new ApObject;
spl_autoload_register([$object, 'fetch']);
spl_autoload_register($object);
$calls = 0;
$closure = function ($name) use (&$calls) { ++$calls; echo "closure:$calls:$name\n"; };
spl_autoload_register($closure);
ap_describe();
$snapshot = spl_autoload_functions();
$snapshot[0] = 'not a loader';
spl_autoload_call('ApFirstMissing');
var_dump(spl_autoload_unregister(['apbase', 'fetch']));
var_dump(spl_autoload_unregister('ap_function'));
ap_describe();
spl_autoload_call('ApSecondMissing');
var_dump($calls);
