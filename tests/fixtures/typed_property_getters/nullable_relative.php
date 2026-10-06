<?php
class GetterScopeBase {
    protected $link = null;
    function set($v) { $this->link = $v; }
    function same(): ?self { return $this->link; }
}
class GetterScopeChild extends GetterScopeBase {
    function base(): ?parent { return $this->link; }
    function called(): ?static { return $this->link; }
}
trait GetterScopeTrait { public $link; function same(): self { return $this->link; } }
class GetterTraitA { use GetterScopeTrait; } class GetterTraitB { use GetterScopeTrait; }
$x = new GetterScopeChild; $base = new GetterScopeBase;
for ($i = 0; $i < 3; $i++) echo ($x->same() === null) + ($x->base() === null) + ($x->called() === null), '|';
$x->set($x); for ($i = 0; $i < 3; $i++) echo ($x->same() === $x) + ($x->base() === $x) + ($x->called() === $x), '|';
$x->set($base); echo ($x->same() === $base) + ($x->base() === $base), '|'; try { $x->called(); } catch (TypeError $e) { echo 'static-error|'; }
$a = new GetterTraitA; $b = new GetterTraitB; $a->link = $a; $b->link = $b;
for ($i = 0; $i < 3; $i++) echo ($a->same() === $a) + ($b->same() === $b), '|';
$a->link = $b; try { $a->same(); } catch (TypeError $e) { echo 'trait-error|'; }
