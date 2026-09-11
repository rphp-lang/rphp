<?php
class NameOwnerBase {
    public static function advance(int &$number): int { return ++$number; }
    public static function owner(): string { return static::class; }
}
class NameOwnerChild extends NameOwnerBase {
    public static function scopes(): string {
        return self::owner() . ':' . parent::owner() . ':' . static::owner();
    }
}
$number = 2;
$alias =& $number;
for ($i = 0; $i < 4; ++$i) {
    echo NameOwnerChild::advance($alias), ',';
}
echo $number, "\n", NameOwnerChild::scopes(), "\n";
class_alias(NameOwnerChild::class, 'NameOwnerAlias');
echo NameOwnerAlias::owner(), "\n";
$method = 'advance';
$class = 'DeferredNameOwner';
spl_autoload_register(function ($requested) use (&$method, &$class) {
    echo 'load:', $requested, "\n";
    $method = 'replaced';
    $class = 'UnrelatedName';
    if ($requested === 'DeferredNameOwner') {
        class DeferredNameOwner extends NameOwnerBase {}
    }
});
echo DeferredNameOwner::advance($number), ':', $class, ':', $method, "\n";
$method = 'advance';
echo DeferredNameOwner::$method($number), "\n";
for ($i = 0; $i < 4; ++$i) {
    try { NameOwnerChild::advance('not-writable'); }
    catch (Error $error) { echo get_class($error), ':', $number, "\n"; }
}
echo NameOwnerChild::advance($number), "\n";
