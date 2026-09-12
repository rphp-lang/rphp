<?php
namespace RegistryÉ;
class Root {
    public function MiXeD() { return 'root'; }
    public function Changed() { return 'base'; }
    protected function Hidden() { return 'protected'; }
    private function Secret() { return 'private'; }
    public function Scope() { return $this->Secret() . ':' . $this->Hidden(); }
    public static function Tag() { return static::class; }
    public string $item { get => 'base-hook'; }
}
class Middle extends Root {
    public function cHANGED() { return 'middle'; }
}
class Leaf extends Middle {
    public function ParentCall() { return parent::MIXED() . ':' . parent::changed(); }
}
class HookLeaf extends Middle {
    public string $item { get => 'child-hook'; }
}
class_alias(Leaf::class, __NAMESPACE__ . '\\AliasLeaf');
foreach ([new Root, new Middle, new Leaf, new AliasLeaf, new HookLeaf] as $value) {
    echo $value->mixed(), ':', $value->CHANGED(), ':', $value->scope(), ':', $value->item, "\n";
    echo $value::TAG(), "\n";
    foreach (['mixed', 'changed', 'scope'] as $method) {
        echo (new \ReflectionMethod($value, $method))->getDeclaringClass()->getName(), '|';
    }
    echo "\n";
    try { $value->hidden(); } catch (\Error $error) { echo "protected-blocked\n"; }
    try { $value->secret(); } catch (\Error $error) { echo "private-blocked\n"; }
}
echo (new Leaf)->parentcall(), "\n";
