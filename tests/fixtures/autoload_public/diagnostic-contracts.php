<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
function ap_attempt($label, $action) {
    echo "case:$label\n";
    try { var_dump($action()); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
function ap_loader($name) { echo "load:$name\n"; }
function ap_other_loader($name) { echo "other:$name\n"; }
ap_attempt('clear-empty', fn() => spl_autoload_unregister('spl_autoload_call'));
ap_attempt('invalid-before-notice', fn() => spl_autoload_register('ap_missing_loader', false));
ap_attempt('forbidden-before-notice', fn() => spl_autoload_register('spl_autoload_call', false));
ap_attempt('false-throw', fn() => spl_autoload_register('ap_loader', false));
ap_attempt('bad-throw', fn() => spl_autoload_register('ap_loader', []));
ap_attempt('bad-prepend', fn() => spl_autoload_register('ap_loader', false, []));
ap_attempt('named', fn() => spl_autoload_register(prepend: true, callback: 'AP_LOADER', throw: true));
var_dump(spl_autoload_functions());
foreach (['class_implements', 'class_uses', 'class_parents'] as $probe) {
    ap_attempt($probe . ':type', fn() => $probe(3));
    ap_attempt($probe . ':true', fn() => $probe(true));
    ap_attempt($probe . ':false', fn() => $probe(false));
    ap_attempt($probe . ':no-load', fn() => $probe('ApAbsent', false));
    ap_attempt($probe . ':load', fn() => $probe('ApAbsent'));
    ap_attempt($probe . ':bad-flag', fn() => $probe('ApAbsent', []));
}
$saved = 'untouched';
set_error_handler(function ($level, $message) { echo "throwing:$level:", count(spl_autoload_functions()), "\n"; throw new Exception('diagnostic interrupted'); });
ap_attempt('named-notice-transaction', function () use (&$saved) { $saved = spl_autoload_register('ap_other_loader', false); return $saved; });
var_dump($saved, spl_autoload_functions());
ap_attempt('notice-transaction', function () use (&$saved) { $saved = spl_autoload_register(null, false); return $saved; });
var_dump($saved, spl_autoload_functions());
ap_attempt('clear-transaction', function () use (&$saved) { $saved = spl_autoload_unregister('spl_autoload_call'); return $saved; });
var_dump($saved, spl_autoload_functions());
ap_attempt('probe-transaction', function () use (&$saved) { $saved = class_uses('ApAbsent', false); return $saved; });
var_dump($saved);
restore_error_handler();
ap_attempt('clear-populated', fn() => spl_autoload_unregister('spl_autoload_call'));
var_dump(spl_autoload_functions());
