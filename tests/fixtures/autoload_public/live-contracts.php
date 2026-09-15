<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
function ap_reset() { foreach (spl_autoload_functions() as $callback) spl_autoload_unregister($callback); }
function ap_tail($name) { echo "tail:$name\n"; }
function ap_added($name) { echo "added:$name\n"; }
function ap_front($name) { echo "front:$name\n"; }
function ap_mutate($name) {
    echo "mutate:$name\n";
    spl_autoload_unregister('ap_tail');
    spl_autoload_register('ap_added');
    spl_autoload_register('ap_front', true, true);
}
spl_autoload_register('ap_mutate');
spl_autoload_register('ap_tail');
spl_autoload_call('ApLiveOne');
echo "second\n";
spl_autoload_call('ApLiveTwo');
ap_reset();
function ap_remove_self($name) {
    echo "self:$name\n";
    spl_autoload_unregister('ap_remove_self');
    spl_autoload_register('ap_added');
}
spl_autoload_register('ap_remove_self');
spl_autoload_register('ap_tail');
spl_autoload_call('ApSelf');
ap_reset();
function ap_clear($name) {
    echo "clear:$name\n";
    spl_autoload_unregister('spl_autoload_call');
    spl_autoload_register('ap_added');
}
spl_autoload_register('ap_clear');
spl_autoload_register('ap_tail');
spl_autoload_call('ApClear');
echo "after-clear\n";
spl_autoload_call('ApAfterClear');
ap_reset();
$depth = 0;
$events = [];
$reentrant = function ($name) use (&$depth, &$events) {
    $events[] = $name;
    echo "enter:$name:$depth\n";
    if ($depth === 0) {
        ++$depth;
        class_exists($name);
        class_exists('ApNested');
        --$depth;
        throw new Exception('load interrupted');
    }
};
spl_autoload_register($reentrant);
spl_autoload_register('ap_tail');
for ($i = 0; $i < 2; ++$i) {
    try { class_exists('ApRecursive'); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
var_dump($events, $depth);
ap_reset();
function ap_a($name) { echo "a:$name\n"; }
function ap_b($name) {
    echo "b:$name\n";
    if ($name === 'previous') spl_autoload_unregister('ap_a');
    if ($name === 'self') spl_autoload_unregister('ap_b');
    if ($name === 'next') spl_autoload_unregister('ap_c');
    if ($name === 'remove-readd') { spl_autoload_unregister('ap_c'); spl_autoload_register('ap_c'); }
    if ($name === 'append') spl_autoload_register('ap_added');
}
function ap_c($name) { echo "c:$name\n"; }
function ap_d($name) { echo "d:$name\n"; }
foreach (['previous', 'self', 'next', 'remove-readd', 'append'] as $scenario) {
    ap_reset();
    foreach (['ap_a', 'ap_b', 'ap_c', 'ap_d'] as $callback) spl_autoload_register($callback);
    echo "scenario:$scenario\n";
    spl_autoload_call($scenario);
}
