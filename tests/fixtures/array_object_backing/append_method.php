<?php
set_error_handler(static fn() => true);
foreach ([ArrayObject::class, ArrayIterator::class] as $kind) {
    $state = (object)['seed' => 17];
    $view = new $kind($state);
    try { $view->append(23); } catch (Error $error) { echo $error->getMessage(), "\n"; }
    echo count($view), ':', $state->seed, "\n";
}
$root = new ArrayObject([5 => 19]);
$nested = new ArrayObject($root);
$nested->append(29);
echo $root[6], ':', count($nested), "\n";
try { $root->exchangeArray(); } catch (ArgumentCountError $error) { echo $error->getMessage(), "\n"; }
echo $root[6], "\n";
