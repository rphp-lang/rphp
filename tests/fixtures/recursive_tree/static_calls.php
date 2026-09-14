<?php
// Original guards for reuse of a previously resolved scalar static call.
function static_show($value) { echo json_encode($value), "\n"; }
function static_attempt($fn) { try { static_show($fn()); } catch (Throwable $e) { static_show([$e::class, str_replace(__FILE__, 'source.php', $e->getMessage())]); } }
set_error_handler(function($level, $message) { static_show([$level, $message]); return true; });
class CachedScalarBase {
    public static function step(int $value): int { return $value + 1; }
    public static function pair(int $left, int $right): int { return $left + $right; }
    public static function change(int &$value): int { return ++$value; }
    public static function owner(): string { return static::class; }
    public function instance(int $value): int { return $value + 1; }
    public function forward(int $value): int { return self::instance($value); }
}
class CachedScalarChild extends CachedScalarBase {
    public static function parentStep(int $value): int { return parent::step($value); }
}
switch (getenv('RPHP_STATIC_GUARD_CASE')) {
case 'values':
    foreach ([1, 2, '3', [], PHP_INT_MAX, 4] as $value) static_attempt(fn() => CachedScalarBase::step($value));
    $strict = eval('declare(strict_types=1); return function($value) { return CachedScalarBase::step($value); };');
    foreach ([5, '6', 7] as $value) static_attempt(fn() => $strict($value));
    break;
case 'scope':
    class_alias(CachedScalarChild::class, 'CachedScalarAlias');
    foreach ([1, 2, 3] as $value) static_show([CachedScalarChild::step($value), CachedScalarAlias::parentStep($value), CachedScalarChild::owner()]);
    $receiver = new CachedScalarBase();
    foreach ([4, 5] as $value) static_show($receiver->forward($value));
    foreach ([6, 7] as $value) static_attempt(fn() => CachedScalarBase::instance($value));
    break;
case 'arguments':
    $value = 10; $alias =& $value;
    for ($i = 0; $i < 3; $i++) static_show([CachedScalarBase::step($alias), CachedScalarBase::change($alias), $value]);
    for ($i = 0; $i < 3; $i++) {
        static_show(CachedScalarBase::pair(right: 20, left: $i));
        $args = [$i, 30]; static_show(CachedScalarBase::pair(...$args));
    }
    $calls = 0;
    $supply = function() use (&$calls) { ++$calls; return 8; };
    for ($i = 0; $i < 3; $i++) static_show([CachedScalarBase::step($supply()), $calls]);
    break;
case 'fallback':
    class CachedScalarMagic { public static function __callStatic($name, $args) { return [$name, $args]; } }
    for ($i = 0; $i < 3; $i++) static_show(CachedScalarMagic::step($i));
    trait CachedScalarTrait { public static function step(int $value): int { return $value + 1; } }
    class CachedScalarTraitUser { use CachedScalarTrait; }
    for ($i = 0; $i < 3; $i++) static_show(CachedScalarTraitUser::step($i));
    for ($i = 0; $i < 2; $i++) static_show(CachedScalarTrait::step($i));
    foreach ([CachedScalarBase::class, CachedScalarChild::class] as $class) static_show($class::step(9));
    break;
}
