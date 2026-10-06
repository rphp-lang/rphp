<?php
interface FirstTypeScope {}
interface SecondTypeScope {}
class ConcreteTypeScope implements FirstTypeScope, SecondTypeScope {}
class RootTypeScope {}
trait ContextualReturns {
    static function lexical($value): self|int { return $value; }
    static function ancestor($value): parent|false { return $value; }
    static function called($value): static|null { return $value; }
    static function absolute($value): (FirstTypeScope&SecondTypeScope)|null { return $value; }
    static function &reference(&$value): static { return $value; }
}
class FirstOwnerScope extends RootTypeScope { use ContextualReturns; }
class ChildOwnerScope extends FirstOwnerScope {}
class OtherOwnerScope extends RootTypeScope { use ContextualReturns; }
function checkContext($callback, $value) {
    try {
        $result = $callback($value);
        echo is_object($result) ? get_class($result) : gettype($result);
    } catch (TypeError $error) {
        echo get_class($error);
    }
    echo '|';
}
foreach ([FirstOwnerScope::class, ChildOwnerScope::class, OtherOwnerScope::class] as $scope) {
    checkContext([$scope, 'lexical'], new FirstOwnerScope);
    checkContext([$scope, 'ancestor'], new RootTypeScope);
    checkContext([$scope, 'called'], new FirstOwnerScope);
    checkContext([$scope, 'absolute'], new ConcreteTypeScope);
    checkContext([$scope, 'absolute'], new RootTypeScope);
}
$closure = function ($value): self|int { return $value; };
checkContext($closure->bindTo(null, FirstOwnerScope::class), new FirstOwnerScope);
checkContext($closure->bindTo(null, OtherOwnerScope::class), new FirstOwnerScope);
function forwardAlias($value): FutureTypeScopeAlias { return $value; }
$object = new ConcreteTypeScope;
checkContext('forwardAlias', $object);
class_alias(ConcreteTypeScope::class, 'FutureTypeScopeAlias');
checkContext('forwardAlias', $object);
$child = new ChildOwnerScope;
$alias =& ChildOwnerScope::reference($child);
$alias = new ChildOwnerScope;
echo (int) ($alias === $child), '|done';
