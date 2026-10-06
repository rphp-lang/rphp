<?php
interface ResolvedRoot {}
interface ResolvedLeaf extends ResolvedRoot {}
class ResolvedBase implements ResolvedLeaf {
    public string $text = 'plain';
    public function __toString(): string { return $this->text; }
}
class ResolvedChild extends ResolvedBase {}
class ResolvedOther {}
function resolvedCheck($value) {
    echo (int) ($value instanceof ResolvedRoot),
         (int) ($value instanceof ResolvedBase),
         (int) ($value instanceof Stringable),
         (int) ($value instanceof ResolvedPublished), '|';
}
$child = new ResolvedChild;
$other = new ResolvedOther;
foreach ([$child, $child, $other, $other, $child, $child] as $value) resolvedCheck($value);
class_alias(ResolvedBase::class, 'ResolvedPublished');
foreach ([$child, $child, $other, $other, new ResolvedPublished, new ResolvedPublished] as $value) resolvedCheck($value);
$reference =& $child;
resolvedCheck($reference);
resolvedCheck(new class extends ResolvedChild {});
resolvedCheck(42);
resolvedCheck(null);
resolvedCheck(function () {});
$autoloads = 0;
spl_autoload_register(function ($name) use (&$autoloads) { $autoloads++; });
for ($i = 0; $i < 3; $i++) echo (int) ($child instanceof ResolvedMissing);
echo ':', $autoloads, '|';
foreach (['resolvedbase', '\\ResolvedRoot', new ResolvedBase, new ResolvedOther] as $target) echo (int) ($reference instanceof $target);
echo '|';
trait ResolvedScope {
    public static function scope($value) {
        echo (int) ($value instanceof self), (int) ($value instanceof static), '|';
    }
}
class ResolvedScopeBase { use ResolvedScope; }
class ResolvedScopeChild extends ResolvedScopeBase {}
ResolvedScopeChild::scope(new ResolvedScopeBase);
ResolvedScopeChild::scope(new ResolvedScopeChild);
try { $child instanceof self; } catch (Error $error) { echo get_class($error), '|'; }
$reflection = new ReflectionClass(ResolvedChild::class);
$initializations = 0;
$ghost = $reflection->newLazyGhost(function ($object) use (&$initializations) { $initializations++; $object->text = 'lazy'; });
resolvedCheck($ghost);
resolvedCheck($ghost);
echo $initializations, '|';
