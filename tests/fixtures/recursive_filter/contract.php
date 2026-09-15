<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "notice:$level:$message\n"; return true; });
function attempt($label, $body) {
    echo "$label:";
    try { $body(); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
function row($value) { echo json_encode($value), "\n"; }
function keep_large($value, $key, $inner) { return $value >= 4; }
function reference_probe(&$value, &$key, &$inner) {
    row([$value, $key, $inner instanceof Iterator]);
    $value = 90; $key = 'changed'; return true;
}
try {
switch (getenv('RPHP_RECURSIVE_FILTER_CASE')) {
case 'metadata':
    foreach (['CallbackFilterIterator', 'RecursiveFilterIterator', 'RecursiveCallbackFilterIterator', 'ParentIterator'] as $name) {
        $class = new ReflectionClass($name);
        row([$name, $class->getParentClass()->getName(), $class->isAbstract(), $class->isFinal()]);
        foreach (['__construct', 'accept', 'hasChildren', 'getChildren'] as $method) {
            if (!$class->hasMethod($method)) continue;
            $m = $class->getMethod($method); $parameters = [];
            foreach ($m->getParameters() as $p) $parameters[] = [$p->getName(), (string) $p->getType(), $p->isOptional(), $p->isPassedByReference()];
            row([$method, $m->getDeclaringClass()->getName(), $m->isAbstract(), $m->getNumberOfRequiredParameters(), $parameters, (string) $m->getReturnType(), (string) $m->getTentativeReturnType()]);
        }
    }
    break;
case 'callback_forms':
    class Selector {
        public function choose($v, $k, $i) { return $v >= 4; }
        public static function select($v, $k, $i) { return $v >= 4; }
        public function __invoke($v, $k, $i) { return $v >= 4; }
    }
    $object = new Selector;
    foreach (['keep_large', [$object, 'choose'], ['Selector', 'select'], 'Selector::select', $object, fn($v, $k, $i) => $v >= 4] as $callback) {
        $inner = new ArrayIterator(['low' => 2, 'mid' => 4, 'high' => 7]);
        $filter = new CallbackFilterIterator(iterator: $inner, callback: $callback);
        row(iterator_to_array($filter));
        row($filter->getInnerIterator() === $inner);
    }
    break;
case 'lazy_selection':
    $inner = new ArrayIterator(['a' => 2, 'b' => 5, 'c' => 6]);
    $filter = new CallbackFilterIterator($inner, function ($v, $k, $i) use ($inner) { row(['test', $v, $k, $i === $inner]); return $v % 2; });
    row(['new', $filter->valid(), $filter->current(), $filter->key()]);
    $filter->rewind(); row(['first', $filter->valid(), $filter->current(), $filter->key()]);
    row(['cached', $filter->valid(), $filter->current(), $filter->key()]);
    $filter->next(); row(['end', $filter->valid(), $filter->current(), $filter->key()]);
    $filter->rewind(); row(['again', $filter->current()]);
    break;
case 'accept_direct':
    $inner = new ArrayIterator([4]);
    $filter = new CallbackFilterIterator($inner, function ($v, $k, $i) { row(['accept', $v, $k, get_class($i)]); return 'nonempty'; });
    var_dump($filter->accept());
    $filter->rewind(); var_dump($filter->accept());
    $filter->next(); var_dump($filter->accept());
    break;
case 'magic_callback':
    class MagicSelector {
        public function __call($name, $arguments) { row([$name, $arguments[0], $arguments[1], get_class($arguments[2])]); return $arguments[0] > 2; }
    }
    $callback = [new MagicSelector, 'choose'];
    $filter = new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([1, [3, 4]]), function ($v, $k, $i) use ($callback) { return $i->hasChildren() || $callback($v, $k, $i); });
    row(iterator_to_array(new RecursiveIteratorIterator($filter), false));
    $direct = new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([2, 5]), $callback);
    row(iterator_to_array($direct, false));
    break;
case 'callback_identity':
    $calls = 0;
    $callback = function ($v, $k, $i) use (&$calls) { ++$calls; return is_array($v) || $v > 2; };
    $filter = new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([1, [2, 3], [4]]), $callback);
    unset($callback);
    row(iterator_to_array(new RecursiveIteratorIterator($filter), false)); row($calls);
    row(iterator_to_array(new RecursiveIteratorIterator($filter), false)); row($calls);
    break;
case 'recursive_filter':
    class BranchFilter extends RecursiveFilterIterator {
        public function accept(): bool { return $this->hasChildren() || $this->current() >= 5; }
    }
    $filter = new BranchFilter(new RecursiveArrayIterator(['x' => 2, 'branch' => ['a' => 5, 'b' => [8, 1]], 'z' => 7]));
    $tree = new RecursiveIteratorIterator($filter);
    foreach ($tree as $key => $value) row([$tree->getDepth(), $key, $value]);
    break;
case 'recursive_children':
    $inner = new RecursiveArrayIterator(['branch' => [4, 9], 'leaf' => 6]);
    $filter = new RecursiveCallbackFilterIterator($inner, fn($v, $k, $i) => true);
    $filter->rewind();
    $a = $filter->getChildren(); $b = $filter->getChildren();
    row([get_class($a), $a !== $b, $a->getInnerIterator() !== $b->getInnerIterator(), $filter->key(), $filter->hasChildren()]);
    $a->rewind(); row([$a->current(), $a->key(), $filter->key()]);
    $filter->next(); row($filter->hasChildren());
    attempt('leaf-child', fn() => var_dump($filter->getChildren()));
    break;
case 'subclass_factory':
    class CustomRecursiveFilter extends RecursiveCallbackFilterIterator {
        public function __construct($iterator, $callback) { row(['construct', get_class($iterator), func_num_args()]); parent::__construct($iterator, $callback); }
    }
    $filter = new CustomRecursiveFilter(new RecursiveArrayIterator([[6]]), fn($v, $k, $i) => true);
    $filter->rewind(); $child = $filter->getChildren(); row(get_class($child));
    $child->rewind(); row($child->current());
    class CustomAbstractFilter extends RecursiveFilterIterator {
        public function __construct($iterator) { row(['abstract-child', func_num_args()]); parent::__construct($iterator); }
        public function accept(): bool { return true; }
    }
    $filter = new CustomAbstractFilter(new RecursiveArrayIterator([[7]]));
    $filter->rewind(); row(get_class($filter->getChildren()));
    break;
case 'parent_filter':
    $filter = new ParentIterator(new RecursiveArrayIterator(['a' => 3, 'b' => [4, [8]], 'c' => []]));
    foreach ([RecursiveIteratorIterator::LEAVES_ONLY, RecursiveIteratorIterator::SELF_FIRST] as $mode) {
        $tree = new RecursiveIteratorIterator($filter, $mode);
        foreach ($tree as $key => $value) row([$tree->getDepth(), $key, $value]);
        echo "end\n";
    }
    break;
case 'layered_filters':
    class OuterBranches extends RecursiveFilterIterator { public function accept(): bool { return $this->hasChildren() || $this->current() > 2; } }
    class EvenBranches extends RecursiveFilterIterator { public function accept(): bool { return $this->hasChildren() || $this->current() % 2 === 0; } }
    $source = new RecursiveArrayIterator([1, [2, 4, [6, 7]], 8]);
    $filter = new OuterBranches(new EvenBranches($source));
    row(iterator_to_array(new RecursiveIteratorIterator($filter), false));
    break;
case 'invalid_arguments':
    foreach ([[], [new ArrayIterator([])], [null, null], [new ArrayIterator([]), null], [new ArrayIterator([]), []], [new ArrayIterator([]), 'absent_filter'], [new ArrayIterator([]), fn() => true, 9]] as $arguments) {
        attempt('callback', function () use ($arguments) { new CallbackFilterIterator(...$arguments); echo "ok\n"; });
    }
    foreach ([new ArrayIterator([]), null, 1] as $inner) {
        attempt('recursive', function () use ($inner) { new RecursiveCallbackFilterIterator($inner, fn() => true); echo "ok\n"; });
        attempt('parent', function () use ($inner) { new ParentIterator($inner); echo "ok\n"; });
    }
    break;
case 'repeated_constructor':
    $filter = new CallbackFilterIterator(new ArrayIterator([5]), fn() => true);
    $filter->rewind();
    attempt('repeat', function () use ($filter) { $filter->__construct(new ArrayIterator([9]), fn() => false); });
    attempt('invalid-repeat', function () use ($filter) { $filter->__construct(null, null); });
    row([$filter->current(), $filter->key(), $filter->valid()]);
    break;
case 'uninitialized':
    class EmptyCallback extends CallbackFilterIterator { public function __construct() {} }
    class EmptyRecursive extends RecursiveCallbackFilterIterator { public function __construct() {} }
    class EmptyParent extends ParentIterator { public function __construct() {} }
    foreach ([new EmptyCallback, new EmptyRecursive, new EmptyParent] as $object) {
        foreach (['accept', 'current', 'key', 'rewind', 'next', 'valid', 'getInnerIterator'] as $method) attempt($method, fn() => var_dump($object->$method()));
        if ($object instanceof RecursiveIterator) foreach (['hasChildren', 'getChildren'] as $method) attempt($method, fn() => var_dump($object->$method()));
    }
    break;
case 'exception_state':
    $throw = true;
    $filter = new CallbackFilterIterator(new ArrayIterator([2, 4, 6]), function ($v, $k, $i) use (&$throw) { row(['test', $v]); if ($v === 4 && $throw) { $throw = false; throw new Exception('selection'); } return $v > 2; });
    attempt('rewind', fn() => $filter->rewind());
    row([$filter->valid(), $filter->key(), $filter->current()]);
    $filter->next(); row([$filter->valid(), $filter->key(), $filter->current()]);
    $filter->rewind(); row([$filter->key(), $filter->current()]);
    break;
case 'by_reference':
    $value = 4; $source = ['seed' => &$value, 'next' => 7];
    $inner = new ArrayIterator($source);
    $filter = new CallbackFilterIterator($inner, 'reference_probe');
    row(iterator_to_array($filter)); row($source); row($value);
    break;
case 'live_mutation':
    $source = ['a' => 1, 'b' => 4, 'c' => 6];
    $inner = new ArrayIterator($source);
    $filter = new CallbackFilterIterator($inner, function ($v, $k, $i) { if ($k === 'a') $i['b'] = 8; return $v >= 4; });
    row(iterator_to_array($filter)); row($source); row($inner->getArrayCopy());
    break;
case 'reentrant':
    $entered = false; $filter = null;
    $filter = new CallbackFilterIterator(new ArrayIterator([3, 5, 7]), function ($v, $k, $i) use (&$entered, &$filter) { row(['test', $v]); if (!$entered) { $entered = true; $filter->next(); } return true; });
    $filter->rewind(); row([$filter->key(), $filter->current()]);
    $filter->next(); row([$filter->key(), $filter->current()]);
    break;
case 'retirement':
    class RetiringIterator extends ArrayIterator { public function __destruct() { echo "inner-retired\n"; } }
    class RetiringCallback { public function __invoke($v, $k, $i) { return true; } public function __destruct() { echo "callback-retired\n"; } }
    $inner = new RetiringIterator([4]); $callback = new RetiringCallback;
    $filter = new CallbackFilterIterator($inner, $callback);
    unset($inner, $callback); $filter->rewind(); echo "release\n"; unset($filter); echo "released\n";
    break;
case 'callback_cycle':
    class CycleCallback {
        public $filter;
        public function __invoke($v, $k, $i) { return true; }
        public function __destruct() { echo "cycle-retired\n"; }
    }
    $callback = new CycleCallback;
    $filter = new CallbackFilterIterator(new ArrayIterator([8]), $callback);
    $callback->filter = $filter; $weak = WeakReference::create($callback);
    unset($callback, $filter); row($weak->get() !== null); gc_collect_cycles(); row($weak->get() === null);
    break;
case 'clone_and_serialize':
    $filter = new CallbackFilterIterator(new ArrayIterator([4]), 'keep_large');
    attempt('clone', fn() => clone $filter);
    attempt('serialize', fn() => print serialize($filter));
    break;
case 'private_scope':
    class PrivateFactory {
        private function select($v, $k, $i) { return $v > 3; }
        public function make() { return new CallbackFilterIterator(new ArrayIterator([2, 5]), [$this, 'select']); }
    }
    $factory = new PrivateFactory; $filter = $factory->make(); unset($factory);
    row(iterator_to_array($filter, false));
    break;
case 'relative_scope':
    class RelativeFactory {
        protected static function select($v, $k, $i) { return $v > 3; }
        public static function make() { return new CallbackFilterIterator(new ArrayIterator([2, 5]), 'self::select'); }
    }
    $filter = RelativeFactory::make(); echo "constructed\n";
    row(iterator_to_array($filter, false)); row(iterator_to_array($filter, false));
    break;
case 'callback_reference_snapshot':
    $callback = fn($v, $k, $i) => $v > 3;
    $alias = &$callback;
    $filter = new CallbackFilterIterator(new ArrayIterator([2, 5]), $alias);
    $callback = fn($v, $k, $i) => $v < 3;
    row(iterator_to_array($filter, false));
    break;
case 'nested_callback_snapshot':
    class SnapshotChoice {
        public function high($v, $k, $i) { return is_array($v) || $v > 3; }
        public function low($v, $k, $i) { return is_array($v) || $v < 3; }
    }
    $owner = new SnapshotChoice; $method = 'high';
    $callback = [&$owner, &$method];
    $filter = new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([[2, 5], 6]), $callback);
    $method = 'low';
    $filter->rewind(); row($filter->current());
    $child = $filter->getChildren(); row(iterator_to_array($child, false));
    $filter->next(); row([$filter->valid(), $filter->current()]);
    break;
case 'unpack_arity':
    foreach ([[], [null], ['callback' => fn() => true], ['iterator' => new ArrayIterator([])]] as $arguments) {
        attempt('unpack', function () use ($arguments) { new CallbackFilterIterator(...$arguments); });
    }
    foreach ([[], ['x'], ['replacement' => 'y', 'subject' => 'x']] as $arguments) {
        attempt('global', fn() => str_replace(...$arguments));
    }
    break;
case 'nullable_children':
    class NullChildIterator implements RecursiveIterator {
        public function rewind(): void { echo "inner-rewind\n"; }
        public function next(): void {}
        public function valid(): bool { return true; }
        public function current(): mixed { return 5; }
        public function key(): mixed { return 'one'; }
        public function hasChildren(): bool { echo "inner-has\n"; return false; }
        public function getChildren(): ?RecursiveIterator { echo "inner-child\n"; return null; }
    }
    class NullChildFilter extends RecursiveFilterIterator { public function accept(): bool { return true; } }
    $filter = new NullChildFilter(new NullChildIterator);
    row($filter->hasChildren()); attempt('abstract-child', fn() => $filter->getChildren());
    $filter = new RecursiveCallbackFilterIterator(new NullChildIterator, fn() => true);
    row($filter->hasChildren()); attempt('callback-child', fn() => $filter->getChildren());
    $parent = new ParentIterator(new NullChildIterator); row($parent->accept());
    break;
case 'typed_callbacks':
    function typed_filter(int $v, string $k, Iterator $i): bool { row([$v, $k]); return $v > 3; }
    $filter = new CallbackFilterIterator(new ArrayIterator(['entry' => '5']), 'typed_filter');
    row(iterator_to_array($filter));
    eval('declare(strict_types=1); $strictFilter = new CallbackFilterIterator(new ArrayIterator(["strict" => "6"]), "typed_filter");');
    row(iterator_to_array($strictFilter));
    $filter = new CallbackFilterIterator(new ArrayIterator(['invalid' => 'bad']), 'typed_filter');
    attempt('typed', fn() => $filter->rewind()); row([$filter->key(), $filter->current()]);
    break;
case 'reference_warning_throw':
    function warning_filter($v, &$k, $i) { echo "callback-entered\n"; return true; }
    $filter = new CallbackFilterIterator(new ArrayIterator(['first' => 8]), 'warning_filter');
    set_error_handler(function ($level, $message) { throw new Exception('reference-warning'); });
    attempt('warning', fn() => $filter->rewind());
    restore_error_handler(); row([$filter->valid(), $filter->key(), $filter->current()]);
    row($filter->accept());
    break;
case 'relative_instance_scope':
    class BoundFactory {
        private $threshold = 3;
        protected function select($v, $k, $i) { return $v > $this->threshold; }
        public function make() { return new CallbackFilterIterator(new ArrayIterator([2, 5]), 'self::select'); }
    }
    $owner = new BoundFactory; $filter = $owner->make(); unset($owner);
    row(iterator_to_array($filter, false)); row(iterator_to_array($filter, false));
    break;
case 'private_child_scope':
    class ChildScopeFactory {
        private function choose($v, $k, $i) { return is_array($v) || $v > 3; }
        public function make() { return new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([[2, 5]]), [$this, 'choose']); }
    }
    $maker = new ChildScopeFactory; $filter = $maker->make(); unset($maker); $filter->rewind();
    attempt('private-child', fn() => $filter->getChildren()); row($filter->current());
    break;
case 'coercion_retirement':
    class CoercingFilterEntry {
        public function __toString() { global $inner, $filter; echo "convert\n"; unset($inner[0]); $filter->next(); echo "converted\n"; return 'text'; }
        public function __destruct() { echo "retired\n"; }
    }
    function string_filter_selection(string $v, $k, $i) { echo "callback:$v\n"; return true; }
    $inner = new ArrayIterator([new CoercingFilterEntry]); $filter = new CallbackFilterIterator($inner, 'string_filter_selection');
    $filter->rewind(); echo "returned\n"; unset($filter, $inner); echo "end\n";
    break;
case 'argument_retirement':
    class RetiringFilterEntry { public function __destruct() { echo "entry-retired\n"; } }
    $inner = new ArrayIterator([new RetiringFilterEntry]);
    $filter = new CallbackFilterIterator($inner, function ($value, $key, $iterator) use (&$filter) {
        echo "callback\n"; unset($iterator[0]); $filter->next(); echo "callback-end\n"; return true;
    });
    $filter->rewind(); echo "returned\n";
    break;
case 'result_retirement':
    class FilterDecision { public function __destruct() { echo "result-retired\n"; } }
    $filter = new CallbackFilterIterator(new ArrayIterator([6]), function () { echo "callback\n"; return new FilterDecision; });
    $filter->rewind(); echo "returned\n"; row($filter->current());
    break;
case 'coercion_retirement_throw':
    class ThrowingCoercedEntry {
        public function __toString() { global $inner, $filter; unset($inner[0]); $filter->next(); return 'text'; }
        public function __destruct() { echo "retired\n"; throw new Exception('retirement'); }
    }
    function throwing_string_selection(string $v, $k, $i) { echo "callback-entered\n"; return true; }
    $inner = new ArrayIterator([new ThrowingCoercedEntry]); $filter = new CallbackFilterIterator($inner, 'throwing_string_selection');
    attempt('rewind', fn() => $filter->rewind()); row([$filter->valid(), $filter->current()]);
    break;
case 'failed_child_retirement':
    class FailedChildFilter extends RecursiveFilterIterator {
        public static $calls = 0;
        public function __construct($iterator) {
            ++self::$calls; echo 'construct:', self::$calls, "\n";
            if (self::$calls === 2) throw new Exception('child');
            parent::__construct($iterator);
        }
        public function accept(): bool { return true; }
        public function __destruct() { echo "destructor\n"; }
    }
    $filter = new FailedChildFilter(new RecursiveArrayIterator([[4]])); $filter->rewind();
    attempt('child', fn() => $filter->getChildren());
    echo "parent-alive\n"; unset($filter); echo "end\n";
    break;
case 'constructor_cache_contract':
    class CachedFilterPayload {
        public int $number;
        public int|string $variant;
        public float $wide;
        public function __construct($number, $variant, $wide) {
            $this->number = $number; $this->variant = $variant; $this->wide = $wide;
        }
    }
    foreach ([[1, 2, 3.5], [4, 5, 6.5], ['7', 'name', 8], [PHP_INT_MIN, PHP_INT_MAX, 9.5]] as $arguments) {
        $value = new CachedFilterPayload($arguments[0], $arguments[1], $arguments[2]);
        row([$value->number, $value->variant, $value->wide, $arguments]);
    }
    $number = 11; $alias =& $number;
    for ($i = 0; $i < 2; ++$i) {
        $value = new CachedFilterPayload($alias, $alias, 12.5);
        row([$value->number, $value->variant, $number]);
    }
    attempt('invalid', fn() => new CachedFilterPayload('wrong', 13, 14.5));
    row([$value->number, $value->variant, $number]);
    break;
case 'child_argument_retirement':
    // Make exception-owned argument lifetime independent of php.ini defaults.
    ini_set('zend.exception_ignore_args', '1');
    class FilterChildInput extends RecursiveArrayIterator {
        public $label;
        public static $mode;
        public function __construct($label, $values) { $this->label = $label; parent::__construct($values); }
        public function getChildren(): ?RecursiveArrayIterator { return new FilterChildInput('argument', [1]); }
        public function __destruct() {
            echo 'retire:', $this->label, "\n";
            if ($this->label === 'argument' && in_array(self::$mode, ['argument', 'both'])) throw new Exception('argument');
        }
    }
    class FilterChildSink extends RecursiveFilterIterator {
        public static $calls = 0;
        public $id;
        public function __construct($iterator) {
            $this->id = ++self::$calls; echo 'construct:', $this->id, "\n";
            if ($this->id === 1) parent::__construct($iterator);
            elseif (in_array(FilterChildInput::$mode, ['constructor', 'both'])) throw new Exception('constructor');
        }
        public function accept(): bool { return true; }
        public function __destruct() { echo 'filter:', $this->id, "\n"; }
    }
    foreach (['normal', 'constructor', 'argument', 'both'] as $mode) {
        echo "mode:$mode\n"; FilterChildInput::$mode = $mode; FilterChildSink::$calls = 0;
        $filter = new FilterChildSink(new FilterChildInput('root', [[1]])); $filter->rewind();
        try { $child = $filter->getChildren(); echo "returned\n"; unset($child); }
        catch (Throwable $e) { echo 'caught:', $e->getMessage(), ':previous:', $e->getPrevious()?->getMessage() ?? 'none', "\n"; }
        unset($filter); echo "end\n";
    }
    break;
case 'callback_argument_errors':
    ini_set('zend.exception_ignore_args', '1');
    class CallbackRetiredInput {
        public static $mode;
        public function __destruct() {
            echo "argument-retired\n";
            if (in_array(self::$mode, ['argument', 'both'])) throw new Exception('argument');
        }
    }
    foreach (['normal', 'callback', 'argument', 'both'] as $mode) {
        echo "mode:$mode\n"; CallbackRetiredInput::$mode = $mode;
        $inner = new ArrayIterator([new CallbackRetiredInput]);
        $filter = new CallbackFilterIterator($inner, function ($value, $key, $iterator) use (&$filter, $mode) {
            echo "callback\n"; unset($iterator[0]); $filter->next();
            if (in_array($mode, ['callback', 'both'])) throw new Exception('callback');
            return true;
        });
        try { $filter->rewind(); echo "returned\n"; }
        catch (Throwable $e) { echo 'caught:', $e->getMessage(), ':previous:', $e->getPrevious()?->getMessage() ?? 'none', "\n"; }
        unset($filter, $inner); echo "end\n";
    }
    break;
case 'native_recursive_cursor_contract':
    $cell = 3;
    $native = new RecursiveArrayIterator(['first' => &$cell, '' => [4]]);
    $filter = new CallbackFilterIterator($native, function ($value, $key, $inner) use (&$cell) {
        row(['accept', $value, $key, $inner instanceof RecursiveArrayIterator]);
        if ($key === 'first') { $cell = 8; $inner['last'] = 9; }
        return true;
    });
    row(iterator_to_array($filter)); row($cell);
    class ObservedRecursiveCursor extends RecursiveArrayIterator {
        public function rewind(): void { echo "rewind\n"; parent::rewind(); }
        public function valid(): bool { $v = parent::valid(); row(['valid', $v]); return $v; }
        public function current(): mixed { echo "current\n"; return parent::current(); }
        public function key(): string|int|null { echo "key\n"; return 'seen:' . parent::key(); }
        public function next(): void { echo "next\n"; parent::next(); }
    }
    $observed = new ObservedRecursiveCursor(['one' => [5], 'two' => [6]]);
    $cache = new RecursiveCachingIterator($observed, CachingIterator::FULL_CACHE);
    foreach ($cache as $key => $value) row(['row', $key, $value, $cache->hasNext()]);
    row($cache->getCache());
    break;
case 'child_constructor_throw':
    class ThrowingChild extends RecursiveFilterIterator {
        public static $calls = 0;
        public function __construct($iterator) {
            ++self::$calls; row(['construct', self::$calls]);
            if (self::$calls === 2) throw new Exception('child-construction');
            parent::__construct($iterator);
        }
        public function accept(): bool { return true; }
    }
    $filter = new ThrowingChild(new RecursiveArrayIterator(['branch' => [9]]));
    $filter->rewind(); attempt('child', fn() => $filter->getChildren());
    row([$filter->key(), $filter->current(), $filter->hasChildren()]);
    $child = $filter->getChildren(); $child->rewind(); row([get_class($child), $child->current()]);
    break;
}
} catch (Throwable $e) { echo 'outer:', get_class($e), ':', $e->getMessage(), "\n"; }
