<?php
// Independent delegation specimens; one fresh request per selected case.
set_error_handler(function ($level, $message) {
    echo 'diagnostic:', $message, "\n";
    return true;
});
function attempt($operation) {
    try { $operation(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
function state($iterator) {
    echo json_encode([$iterator->valid(), $iterator->key(), $iterator->current()]), "\n";
}
class RecordedIterator implements Iterator {
    public int $position = 0;
    public string $fail = '';
    function __construct(public array $items = [13, 27, 41]) {}
    function mark($name) {
        echo $name, ':', $this->position, "\n";
        if ($this->fail === $name) throw new Exception('callback-' . $name);
    }
    function rewind(): void { $this->mark('rewind'); $this->position = 0; }
    function next(): void { $this->mark('next'); ++$this->position; }
    function valid(): bool { $this->mark('valid'); return isset($this->items[$this->position]); }
    function current(): mixed { $this->mark('current'); return $this->items[$this->position] ?? null; }
    function key(): mixed { $this->mark('key'); return 'item-' . $this->position; }
}
switch (getenv('RPHP_DELEGATION_CASE')) {
case 'abstract-metadata':
    foreach ([new ReflectionMethod(OuterIterator::class,'getInnerIterator'), new ReflectionMethod(Iterator::class,'current'), new ReflectionMethod(IteratorIterator::class,'current')] as $method) {
        var_dump($method->isAbstract(), $method->getModifiers());
    }
    foreach ((new ReflectionClass(OuterIterator::class))->getMethods(ReflectionMethod::IS_ABSTRACT) as $method) echo $method->getName(), "\n";
    attempt(function () { Iterator::current(); });
    attempt(function () { OuterIterator::getInnerIterator(); });
    break;
case 'forwarding':
    class ExtraOperations extends ArrayIterator {
        function adjust(int &$number, int $step = 3): int { $number += $step; return $this->count(); }
        protected function secret(): void { echo 'unexpected-secret'; }
        function __destruct() { echo "extra-release\n"; }
    }
    $outer = new IteratorIterator(new ExtraOperations([4,8]));
    var_dump($outer->count(), method_exists($outer, 'count'), is_callable([$outer,'count']));
    $number = 10; var_dump($outer->adjust(step: 5, number: $number), $number);
    $method = 'adjust'; var_dump($outer->$method($number), $number);
    $callback = [$outer,'adjust']; var_dump(call_user_func_array($callback,[&$number,2]), $number);
    attempt(function () use ($outer) { $outer->secret(); });
    attempt(function () use ($outer) { $outer->missing(); });
    class MagicOuter extends IteratorIterator {
        function __call($name,$arguments) { echo "outer-magic:$name\n"; return 99; }
    }
    var_dump((new MagicOuter(new ArrayIterator([1])))->count());
    class ForwardingOuter extends IteratorIterator {
        function __destruct() { echo 'outer-release:', $this->count(), "\n"; }
    }
    $owned = new ForwardingOuter(new ExtraOperations([2,5,9])); unset($owned);
    // Retire both explicit PHP owners here; request-global callback-array
    // shutdown accounting is a separately recorded baseline limitation.
    unset($callback, $outer);
    break;
case 'construction-reentry':
    class DeferredDelegate extends IteratorIterator {
        function __construct() {}
        function initialize($inner) { parent::__construct($inner); }
    }
    class InitializingAggregate implements IteratorAggregate {
        public bool $throw = false;
        function __construct(public DeferredDelegate $owner) {}
        function getIterator(): Traversable {
            echo "aggregate-enter\n";
            attempt(function () { $this->owner->initialize(new ArrayIterator([91])); });
            attempt(function () { state($this->owner); });
            if ($this->throw) throw new Exception('aggregate-failure');
            return new ArrayIterator([37]);
        }
    }
    foreach ([false,true] as $fail) {
        echo $fail ? "throwing\n" : "returning\n";
        $outer = new DeferredDelegate; $aggregate = new InitializingAggregate($outer); $aggregate->throw = $fail;
        attempt(function () use ($outer,$aggregate) { $outer->initialize($aggregate); });
        attempt(function () use ($outer) { $outer->rewind(); state($outer); });
        attempt(function () use ($outer) { $outer->initialize(new ArrayIterator([73])); });
        attempt(function () use ($outer) { state($outer); });
    }
    break;
case 'uninitialized-forwarding':
    class EmptyDelegate extends IteratorIterator { function __construct() {} }
    $outer = new EmptyDelegate;
    attempt(function () use ($outer) { $outer->absent(); });
    attempt(function () use ($outer) { var_dump(method_exists($outer,'absent')); });
    attempt(function () use ($outer) { var_dump(is_callable([$outer,'absent'])); });
    attempt(function () use ($outer) { call_user_func([$outer,'absent']); });
    break;
case 'interface-projection':
    interface RootProjection {}
    interface MiddleProjection extends RootProjection {}
    interface OuterProjection extends MiddleProjection {}
    interface OtherProjection {}
    class BaseProjection implements OuterProjection, OtherProjection {}
    class ChildProjection extends BaseProjection {}
    class FinalProjection extends ChildProjection {}
    class CustomIteratorProjection extends IteratorIterator {}
    class CustomLimitProjection extends LimitIterator {}
    foreach ([MiddleProjection::class,OuterProjection::class,BaseProjection::class,ChildProjection::class,FinalProjection::class,CustomIteratorProjection::class,CustomLimitProjection::class] as $name) {
        echo $name, ':', implode(',', (new ReflectionClass($name))->getInterfaceNames()), "\n";
    }
    break;
case 'cache':
    foreach (['IteratorIterator', 'LimitIterator', 'NoRewindIterator'] as $class) {
        echo $class, "\n";
        $inner = new RecordedIterator;
        $outer = new $class($inner);
        echo "constructed\n"; state($outer); state($outer);
        $outer->rewind(); state($outer); state($outer);
        $inner->items[0] = 99; $inner->position = 1;
        echo "inner-mutated\n"; state($outer);
        $outer->next(); state($outer);
        var_dump($outer->getInnerIterator() === $inner);
    }
    break;
case 'native-cache':
    foreach (['IteratorIterator', 'LimitIterator', 'NoRewindIterator'] as $class) {
        echo $class, "\n";
        $inner = new ArrayIterator(['red' => ['n' => 2], 'blue' => 7]);
        $outer = new $class($inner);
        state($outer); $outer->rewind(); state($outer);
        $inner['red'] = ['n' => 9]; state($outer);
        $copy = $outer->current(); if (is_array($copy)) $copy['n'] = 44;
        state($outer); $inner->next(); state($outer);
        $outer->next(); state($outer); $outer->rewind(); state($outer);
    }
    break;
case 'limits':
    foreach ([[0,-1], [1,1], [1,0], [5,2]] as [$offset,$count]) {
        echo "limits:$offset:$count\n";
        $inner = new ArrayIterator(['a'=>3, 'b'=>5, 'c'=>7]);
        $outer = new LimitIterator($inner, $offset, $count);
        state($outer); var_dump($outer->getPosition());
        attempt(function () use ($outer) { $outer->rewind(); });
        state($outer); var_dump($outer->getPosition());
        foreach ([0,1,2,3,-1,1] as $seek) {
            echo "seek:$seek\n";
            attempt(function () use ($outer, $seek) { var_dump($outer->seek($seek)); });
            state($outer); state($inner); var_dump($outer->getPosition());
        }
        $outer->next(); state($outer); var_dump($outer->getPosition());
    }
    foreach ([[-1,-1],[0,-2]] as [$offset,$count]) {
        attempt(function () use ($offset,$count) { new LimitIterator(new ArrayIterator, $offset, $count); });
    }
    break;
case 'seek-protocol':
    $inner = new RecordedIterator;
    $outer = new LimitIterator($inner, 1, 2);
    $outer->rewind(); state($outer);
    foreach ([2,1,9,2] as $position) {
        attempt(function () use ($outer, $position) { var_dump($outer->seek($position)); });
        state($outer);
    }
    class RecordedSeek extends RecordedIterator implements SeekableIterator {
        function seek(int $offset): void { echo "seek:$offset\n"; $this->position = $offset; }
    }
    $outer = new LimitIterator(new RecordedSeek, 1, 2);
    $outer->rewind(); state($outer); $outer->seek(2); state($outer);
    $outer->seek(1); state($outer);
    break;
case 'uninitialized':
    class BareIterator extends IteratorIterator { function __construct() {} }
    class BareLimit extends LimitIterator { function __construct() {} }
    class BareNoRewind extends NoRewindIterator { function __construct() {} }
    foreach ([new BareIterator, new BareLimit, new BareNoRewind] as $outer) {
        echo get_class($outer), "\n";
        foreach (['valid','key','current','next','rewind','getInnerIterator'] as $method) {
            attempt(function () use ($outer,$method) { var_dump($outer->$method()); });
        }
        if ($outer instanceof LimitIterator) {
            attempt(function () use ($outer) { var_dump($outer->getPosition()); });
            attempt(function () use ($outer) { var_dump($outer->seek(0)); });
        }
    }
    foreach (['IteratorIterator','LimitIterator','NoRewindIterator'] as $class) {
        foreach ([[], 7, null, new stdClass] as $bad) {
            attempt(function () use ($class,$bad) { new $class($bad); });
        }
    }
    break;
case 'reinitialize':
    foreach (['IteratorIterator','LimitIterator','NoRewindIterator'] as $class) {
        echo $class, "\n";
        $inner = new ArrayIterator([11,22]); $outer = new $class($inner);
        $outer->rewind(); $outer->next(); state($outer);
        attempt(function () use ($outer) { $outer->__construct(new ArrayIterator([33])); });
        state($outer); var_dump($outer->getInnerIterator() === $inner);
        attempt(function () use ($outer) { $outer->__construct([]); });
        state($outer);
    }
    break;
case 'aggregates':
    class NestedAggregate implements IteratorAggregate {
        function __construct(public Traversable $inner, public string $name) {}
        function getIterator(): Traversable { echo 'aggregate:', $this->name, "\n"; return $this->inner; }
    }
    $inner = new ArrayIterator(['v'=>14]);
    $aggregate = new NestedAggregate(new NestedAggregate($inner, 'inner'), 'outer');
    $outer = new IteratorIterator($aggregate);
    echo "constructed\n"; var_dump($outer->getInnerIterator() === $inner);
    state($outer); $outer->rewind(); state($outer);
    foreach (['LimitIterator','NoRewindIterator'] as $class) {
        attempt(function () use ($class,$aggregate) { new $class($aggregate); });
    }
    class SelfAggregate implements IteratorAggregate { function getIterator(): Traversable { return $this; } }
    attempt(function () { new IteratorIterator(new SelfAggregate); });
    foreach ([null, 'ArrayIterator', 'MissingIteratorClass', 'stdClass'] as $class) {
        attempt(function () use ($inner,$class) { $outer = new IteratorIterator($inner, $class); $outer->rewind(); state($outer); });
    }
    break;
case 'exceptions':
    foreach (['rewind','valid','current','key','next'] as $failed) {
        echo 'failure:', $failed, "\n";
        $inner = new RecordedIterator; $outer = new IteratorIterator($inner);
        $outer->rewind(); state($outer); $inner->fail = $failed;
        attempt(function () use ($outer,$failed) { if ($failed === 'rewind') $outer->rewind(); else $outer->next(); });
        state($outer); $inner->fail = ''; $outer->next(); state($outer);
    }
    break;
case 'metadata':
    foreach (['OuterIterator','IteratorIterator','LimitIterator','NoRewindIterator'] as $class) {
        $reflection = new ReflectionClass($class);
        echo $class, ':', implode(',', $reflection->getInterfaceNames()), "\n";
        foreach ($reflection->getMethods() as $method) {
            echo $method->getDeclaringClass()->getName(), '::', $method->getName(), '(';
            foreach ($method->getParameters() as $parameter) {
                echo $parameter->hasType() ? (string)$parameter->getType() : '-', ' $', $parameter->getName();
                if ($parameter->isDefaultValueAvailable()) echo '=', json_encode($parameter->getDefaultValue());
                echo ';';
            }
            echo ')', $method->hasTentativeReturnType() ? (string)$method->getTentativeReturnType() : '-', "\n";
        }
        echo 'properties:', count($reflection->getProperties()), "\n";
    }
    $outer = new IteratorIterator(new ArrayIterator([4])); $outer->rewind();
    var_dump((array)$outer, get_object_vars($outer));
    break;
case 'release':
    class ReleasedItem {
        function __construct(public string $name) {}
        function __destruct() { echo 'release:', $this->name, "\n"; }
    }
    class ProducedIterator implements Iterator {
        public int $position = 0;
        function rewind(): void { echo "rewind\n"; $this->position = 0; }
        function next(): void { echo "next\n"; ++$this->position; }
        function valid(): bool { echo "valid\n"; return $this->position < 2; }
        function current(): mixed { echo "current\n"; return new ReleasedItem('v' . $this->position); }
        function key(): mixed { echo "key\n"; return $this->position; }
        function __destruct() { echo "release:inner\n"; }
    }
    foreach (['IteratorIterator','LimitIterator','NoRewindIterator'] as $class) {
        echo $class, "\n";
        $outer = new $class(new ProducedIterator);
        $outer->rewind(); $value = $outer->current(); unset($value);
        echo "before-next\n"; $outer->next(); echo "after-next\n";
        $value = $outer->current(); unset($value);
        echo "before-unset\n"; unset($outer); echo "after-unset\n";
    }
    break;
case 'references-cycles':
    class OrdinaryCloneBase {
        public int $number = 19;
        function plus(int $offset): int { return $this->number + $offset; }
    }
    class OrdinaryCloneChild extends OrdinaryCloneBase {}
    $ordinary = new OrdinaryCloneChild;
    $copy = clone $ordinary; $copy->number = 31;
    echo $ordinary->number, ':', $copy->number, "\n";
    class_alias(OrdinaryCloneChild::class, 'DelegationLookupAlias');
    $alias = new dElEgAtIoNlOoKuPaLiAs;
    $method = 'pLuS';
    var_dump($alias->$method(4), call_user_func([$alias, 'PLUS'], 5));
    attempt(function () use ($alias) { $alias->unregistered(); });
    class UninitializedCloneDelegate extends IteratorIterator { function __construct() {} }
    attempt(function () { $copy = clone new UninitializedCloneDelegate; });
    $slot = ['value'=>1]; $inner = new ArrayIterator(['ref'=>&$slot]);
    $outer = new IteratorIterator($inner); $outer->rewind();
    $slot = ['value'=>8]; state($outer);
    $copy = $outer->current(); $copy['value'] = 55;
    echo json_encode([$slot,$inner->current(),$outer->current()]), "\n";
    attempt(function () use ($outer) { foreach ($outer as &$value) echo 'unexpected-reference'; });
    foreach (['IteratorIterator','LimitIterator','NoRewindIterator'] as $class) {
        echo $class, "\n";
        $outer = new $class(new ArrayIterator([1]));
        attempt(function () use ($outer) { $copy = clone $outer; echo "cloned\n"; });
        unset($outer);
    }
    class CycleInner extends ArrayIterator { function __destruct() { echo "cycle-release\n"; } }
    $inner = new CycleInner;
    $outer = new IteratorIterator($inner); $inner['back'] = $outer;
    $weak = WeakReference::create($outer);
    unset($outer,$inner); gc_collect_cycles(); var_dump($weak->get());
    break;
case 'overrides':
    class OuterOverride extends IteratorIterator {
        function rewind(): void { echo "outer-rewind\n"; parent::rewind(); }
        function current(): mixed { echo "outer-current\n"; return parent::current() * 2; }
        function key(): mixed { echo "outer-key\n"; return 'wrapped-' . parent::key(); }
        function next(): void { echo "outer-next\n"; parent::next(); }
        function valid(): bool { echo "outer-valid\n"; return parent::valid(); }
    }
    $outer = new OuterOverride(new ArrayIterator(['a'=>4,'b'=>9]));
    foreach (new LimitIterator($outer, 1, 1) as $key=>$value) echo $key, ':', $value, "\n";
    $inner = new ArrayIterator([2,6,10]); $inner->next();
    $outer = new NoRewindIterator($inner);
    foreach ($outer as $key=>$value) { echo $key, ':', $value, "\n"; break; }
    foreach ($outer as $key=>$value) echo $key, ':', $value, "\n";
    foreach ($outer as $key=>$value) echo 'unexpected:', $value;
    break;
}
