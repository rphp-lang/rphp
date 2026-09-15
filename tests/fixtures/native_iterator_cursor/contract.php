<?php
error_reporting(E_ALL);
function emit($value) { echo json_encode($value), "\n"; }
set_error_handler(function ($level, $message) { emit(['diagnostic', $level, $message]); return true; });
function attempt($body) {
    try { $body(); } catch (Throwable $error) { emit([get_class($error), $error->getMessage()]); }
}
class Steps implements Iterator {
    public $at = 0;
    public $hook = null;
    public function __construct(public $label, public $values) {}
    public function rewind(): void { emit([$this->label, 'rewind']); $this->at = 0; }
    public function valid(): bool {
        emit([$this->label, 'valid', $this->at]);
        if ($this->hook !== null) { $f = $this->hook; $this->hook = null; $f(); }
        return $this->at < count($this->values);
    }
    public function current(): mixed { emit([$this->label, 'current']); return $this->values[$this->at]; }
    public function key(): mixed { emit([$this->label, 'key']); return $this->label . $this->at; }
    public function next(): void { emit([$this->label, 'next']); $this->at++; }
}
switch (getenv('RPHP_NATIVE_ITERATOR_CURSOR_CASE')) {
case 'empty_operations':
    $cursor = new EmptyIterator;
    emit([$cursor->valid(), $cursor->rewind(), $cursor->next(), $cursor->valid()]);
    attempt(function () use ($cursor) { $cursor->current(); });
    attempt(function () use ($cursor) { $cursor->key(); });
    emit(iterator_to_array(new LimitIterator($cursor, 0, 4)));
    break;
case 'empty_override':
    class ObservedEmpty extends EmptyIterator {
        public function rewind(): void { echo "rewind\n"; parent::rewind(); }
        public function valid(): false { echo "valid\n"; return parent::valid(); }
    }
    foreach (new ObservedEmpty as $entry) { echo "unreachable\n"; }
    emit((new ObservedEmpty) instanceof Iterator);
    break;
case 'infinite_callbacks':
    $cursor = new InfiniteIterator(new Steps('turn', ['a', 'b']));
    emit([$cursor->valid(), $cursor->current(), $cursor->key()]);
    $cursor->rewind();
    for ($i = 0; $i < 5; $i++) {
        emit([$cursor->valid(), $cursor->key(), $cursor->current()]);
        $cursor->next();
    }
    break;
case 'infinite_empty':
    $cursor = new InfiniteIterator(new EmptyIterator);
    $cursor->rewind(); $cursor->next();
    emit([$cursor->valid(), $cursor->current(), $cursor->key()]);
    emit(iterator_to_array(new LimitIterator($cursor, 0, 3)));
    break;
case 'infinite_composition':
    $inner = new ArrayIterator(['x', 'y', 'z', 'w']);
    $cursor = new LimitIterator(new InfiniteIterator(new LimitIterator($inner, 1, 2)), 1, 5);
    $rows = [];
    foreach ($cursor as $key => $value) { $rows[] = [$key, $value]; }
    emit($rows);
    break;
case 'infinite_constructor':
    $cursor = new InfiniteIterator(new ArrayIterator([3]));
    emit($cursor->getInnerIterator() instanceof ArrayIterator);
    attempt(function () use ($cursor) { $cursor->__construct(new ArrayIterator([4])); });
    attempt(function () { new InfiniteIterator(new stdClass); });
    class Unready extends InfiniteIterator { public function __construct() {} }
    attempt(function () { (new Unready)->next(); });
    break;
case 'infinite_callback_exception':
    $inner = new Steps('throw', [9]); $cursor = new InfiniteIterator($inner);
    $cursor->rewind();
    $inner->hook = function () { throw new LogicException('probe'); };
    attempt(function () use ($cursor) { $cursor->next(); });
    emit([$cursor->valid(), $cursor->current(), $cursor->key()]);
    $cursor->rewind(); emit($cursor->current());
    break;
case 'infinite_clone':
    $cursor = new InfiniteIterator(new ArrayIterator(['a', 'b']));
    $cursor->rewind();
    attempt(function () use ($cursor) { $copy = clone $cursor; emit([$copy->valid(), $copy->current()]); });
    break;
case 'multiple_flags':
    foreach ([0, 1, 2, 3, 7, -1] as $flags) {
        $cursor = new MultipleIterator($flags);
        $cursor->attachIterator(new ArrayIterator(['a', 'b']), 'left');
        $cursor->attachIterator(new ArrayIterator([7]), 'right');
        $rows = [];
        foreach ($cursor as $key => $value) { $rows[] = [$key, $value]; }
        emit([$cursor->getFlags(), $rows]);
    }
    break;
case 'multiple_empty':
    $cursor = new MultipleIterator;
    emit([$cursor->getFlags(), $cursor->valid(), $cursor->countIterators()]);
    attempt(function () use ($cursor) { $cursor->current(); });
    attempt(function () use ($cursor) { $cursor->key(); });
    $cursor->setFlags(0); emit($cursor->valid());
    attempt(function () use ($cursor) { $cursor->current(); });
    attempt(function () use ($cursor) { $cursor->key(); });
    break;
case 'multiple_labels':
    $a = new ArrayIterator(['first']); $b = new ArrayIterator(['second']);
    $cursor = new MultipleIterator(2);
    $cursor->attachIterator($a, '7'); $cursor->attachIterator($b, 7);
    $cursor->rewind(); emit([$cursor->countIterators(), $cursor->current()]);
    attempt(function () use ($cursor, $b) { $cursor->attachIterator($b, '7'); });
    emit($cursor->current());
    $cursor->attachIterator($a, 'changed'); emit([$cursor->countIterators(), $cursor->current()]);
    $cursor->attachIterator($b); attempt(function () use ($cursor) { $cursor->current(); });
    break;
case 'multiple_attach_errors':
    $cursor = new MultipleIterator;
    $inner = new ArrayIterator([8]);
    attempt(function () use ($cursor) { $cursor->attachIterator(new stdClass); });
    attempt(function () use ($cursor, $inner) { $cursor->attachIterator($inner, []); });
    emit($cursor->countIterators());
    $cursor->attachIterator($inner, 'same');
    attempt(function () use ($cursor, $inner) { $cursor->attachIterator($inner, 'same'); });
    emit([$cursor->countIterators(), $cursor->containsIterator($inner), $cursor->detachIterator($inner), $cursor->containsIterator($inner)]);
    break;
case 'multiple_callbacks':
    $cursor = new MultipleIterator;
    $cursor->attachIterator(new Steps('first', [2]));
    $cursor->attachIterator(new Steps('second', [4, 6]));
    $cursor->rewind(); emit($cursor->valid()); emit($cursor->current()); emit($cursor->key());
    $cursor->next(); emit($cursor->valid());
    attempt(function () use ($cursor) { $cursor->current(); });
    $cursor->setFlags(0); emit($cursor->valid()); emit($cursor->current()); emit($cursor->key());
    break;
case 'multiple_mutation':
    $cursor = new MultipleIterator(0);
    $a = new Steps('a', [1]); $b = new Steps('b', [2]); $c = new Steps('c', [3]);
    $cursor->attachIterator($a); $cursor->attachIterator($b);
    $a->hook = function () use ($cursor, $b, $c) { $cursor->detachIterator($b); $cursor->attachIterator($c); };
    $cursor->rewind(); emit($cursor->current()); emit($cursor->countIterators());
    break;
case 'multiple_generators':
    function source($prefix, $count) { for ($i = 0; $i < $count; $i++) { yield $prefix . $i => $i + 10; } }
    $cursor = new MultipleIterator(2);
    $cursor->attachIterator(source('l', 2), 'left');
    $cursor->attachIterator(source('r', 1), 'right');
    foreach ($cursor as $key => $value) { emit([$key, $value]); }
    break;
case 'multiple_clone':
    $a = new ArrayIterator(['x', 'y']); $cursor = new MultipleIterator;
    $cursor->attachIterator($a, 'first'); $cursor->rewind(); $copy = clone $cursor;
    emit([$copy->getFlags(), $copy->countIterators(), $copy->current()]);
    $copy->next(); emit([$cursor->current(), $copy->current()]);
    $copy->detachIterator($a); emit([$cursor->countIterators(), $copy->countIterators()]);
    break;
case 'multiple_lifetime':
    class Dropping extends ArrayIterator {
        public function __destruct() { echo "child-drop\n"; }
    }
    $cursor = new MultipleIterator; $child = new Dropping([1]);
    $weak = WeakReference::create($child); $cursor->attachIterator($child); unset($child);
    emit($weak->get() !== null); $cursor->detachIterator($weak->get()); emit($weak->get());
    break;
case 'multiple_cycle':
    $cursor = new MultipleIterator; $child = new ArrayIterator; $child->owner = $cursor;
    $weak = WeakReference::create($cursor); $cursor->attachIterator($child); unset($cursor, $child);
    gc_collect_cycles(); emit($weak->get());
    break;
case 'multiple_offsets':
    $cursor = new MultipleIterator; $child = new ArrayIterator([5]);
    emit([$cursor instanceof ArrayAccess, $cursor instanceof Countable]);
    attempt(function () use ($cursor) { $cursor[new stdClass] = 'bad'; });
    $cursor[$child] = 'named'; emit($cursor->countIterators());
    attempt(function () use ($cursor, $child) { emit(isset($cursor[$child])); });
    attempt(function () use ($cursor, $child) { emit($cursor[$child]); });
    attempt(function () use ($cursor, $child) { unset($cursor[$child]); });
    emit($cursor->countIterators());
    break;
case 'multiple_debug':
    class DebugParallel extends MultipleIterator { public function __debugInfo(): array { return ['base' => count(parent::__debugInfo())]; } }
    emit((new DebugParallel)->__debugInfo());
    $cursor = new MultipleIterator; $cursor->attachIterator(new ArrayIterator([4]), 'tag');
    $debug = $cursor->__debugInfo();
    emit([array_keys($debug), count(array_values($debug)[0])]);
    break;
case 'multiple_reinitialize':
    $cursor = new MultipleIterator(3); $cursor->attachIterator(new ArrayIterator([2]), 'retained');
    $cursor->__construct(0); emit([$cursor->getFlags(), $cursor->countIterators()]);
    $cursor->rewind(); emit($cursor->current());
    class UnreadyParallel extends MultipleIterator { public function __construct() {} }
    $bare = new UnreadyParallel; emit([$bare->getFlags(), $bare->valid(), $bare->countIterators()]);
    break;
case 'multiple_live_label':
    $cursor = new MultipleIterator(3); $child = new Steps('item', ['v']);
    $cursor->attachIterator($child, 'old');
    $child->hook = function () use ($cursor, $child) { $cursor->attachIterator($child, 'new'); };
    $cursor->rewind(); emit($cursor->current()); emit($cursor->key());
    break;
case 'multiple_live_flags':
    $cursor = new MultipleIterator(1); $child = new Steps('item', ['v']);
    $cursor->attachIterator($child, 'named');
    $child->hook = function () use ($cursor) { $cursor->setFlags(3); };
    $cursor->rewind(); emit($cursor->current()); emit($cursor->getFlags());
    break;
case 'multiple_errors_resume':
    $cursor = new MultipleIterator; $child = new Steps('fail', [11]);
    $cursor->attachIterator($child); $cursor->rewind();
    $child->hook = function () { throw new LogicException('interrupted'); };
    try { $cursor->current(); } catch (Throwable $e) { emit([get_class($e), $e->getMessage(), $e->getPrevious()?->getMessage()]); }
    emit($cursor->current()); emit($cursor->key());
    $cursor->setFlags(0); $child->hook = function () { throw new LogicException('any-failure'); };
    try { $cursor->current(); } catch (Throwable $e) { emit([get_class($e), $e->getMessage(), $e->getPrevious()?->getMessage()]); }
    break;
case 'multiple_value_cow':
    $value = ['seed']; $inner = new ArrayIterator([$value]);
    $cursor = new MultipleIterator; $cursor->attachIterator($inner); $cursor->rewind();
    $first = $cursor->current(); $first[0][] = 'changed';
    emit([$first, $cursor->current(), $value]);
    break;
case 'multiple_native_info':
    $cursor = new MultipleIterator; $child = new ArrayIterator([3]);
    $info = ['raw']; $cursor[$child] = $info; $info[] = 'outside';
    $debug = array_values($cursor->__debugInfo())[0]; emit($debug[0]['inf']);
    $cursor->setFlags(3); $cursor->rewind(); attempt(function () use ($cursor) { $cursor->current(); });
    unset($cursor[$child]); emit($cursor->countIterators());
    break;
case 'multiple_info_conversion':
    class Label { public function __toString(): string { echo "label\n"; return 'word'; } }
    foreach ([false, true, 2.5, new Label] as $info) {
        $cursor = new MultipleIterator(3); $cursor->attachIterator(new ArrayIterator([1]), $info);
        $cursor->rewind(); emit($cursor->current());
    }
    eval('declare(strict_types=1); try { (new MultipleIterator)->attachIterator(new ArrayIterator, 2.5); } catch (Throwable $e) { emit([get_class($e), $e->getMessage()]); }');
    break;
case 'metadata':
    foreach (['EmptyIterator', 'InfiniteIterator', 'MultipleIterator'] as $class) {
        $r = new ReflectionClass($class); $rows = [];
        foreach ($r->getMethods() as $method) {
            if ($method->getDeclaringClass()->name !== $class) { continue; }
            $args = [];
            foreach ($method->getParameters() as $p) { $args[] = [(string)$p->getType(), $p->name, $p->isOptional(), $p->isDefaultValueAvailable() ? $p->getDefaultValue() : 'required']; }
            $rows[] = [$method->name, $args, (string)$method->getTentativeReturnType()];
        }
        emit([$class, $rows]);
    }
    break;
default: throw new LogicException('unknown specimen');
}
