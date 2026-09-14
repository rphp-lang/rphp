<?php
class ScopeRoot {
    public static function read(&$count): string {
        ++$count;
        return self::class . '/' . get_called_class();
    }
    public static function forward(&$count): string { return self::read($count); }
}
class ScopeLeft extends ScopeRoot {}
class ScopeRight extends ScopeRoot {
    public static function forward(&$count): string { return parent::read($count); }
}
class_alias(ScopeLeft::class, 'ScopeAlias');
function concrete(&$count): string { return sCoPeAlIaS::read($count); }
$count = 0;
for ($i = 0; $i < 3; ++$i) {
    echo concrete($count), ':', $count, "\n";
    echo ScopeLeft::forward($count), ':', $count, "\n";
    echo ScopeRight::forward($count), ':', $count, "\n";
}
trait ScopeTrait {
    public static function name(&$count): string {
        ++$count;
        return __CLASS__ . '/' . get_called_class();
    }
    public static function forward(&$count): string { return self::name($count); }
}
class FirstScope { use ScopeTrait; }
class SecondScope { use ScopeTrait; }
foreach ([FirstScope::class, SecondScope::class, FirstScope::class] as $type) {
    echo $type::forward($count), ':', $count, "\n";
}
class MagicScope {
    public static function __callStatic($method, $args) {
        return get_called_class() . '/' . $method;
    }
}
for ($i = 0; $i < 2; ++$i) { echo MagicScope::missing(), "\n"; }
class ScopeReceiver {
    public function read(): string { return get_called_class(); }
    public function forward(): string { return self::read(); }
}
class ScopeReceiverChild extends ScopeReceiver {}
foreach ([new ScopeReceiver, new ScopeReceiverChild, new ScopeReceiver] as $receiver) {
    echo $receiver->forward(), "\n";
}
class ScalarScope {
    public static function successor(int $value): int { return $value + 1; }
}
class_alias(ScalarScope::class, 'ScalarScopeAlias');
function scalar_scope_site($input) {
    $alias =& $input;
    return sCaLaRsCoPeAlIaS::successor($alias);
}
foreach ([4, 5, '6', 7.0, true, [], PHP_INT_MAX, 8] as $input) {
    try { echo 'scalar:', scalar_scope_site($input), "\n"; }
    catch (TypeError $error) { echo "fallback:TypeError\n"; }
}
$sequence = 0;
for ($i = 0; $i < 3; ++$i) {
    echo 'ordered:', ScalarScope::successor(++$sequence), ':', $sequence, "\n";
}
