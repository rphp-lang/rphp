<?php
// Original fixed-slot specimens. Each case runs in a fresh request.
set_error_handler(function ($level, $message) {
    echo "diagnostic:$level:$message\n";
    return true;
});
function attempt($body) {
    try { $body(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
function slots($array) {
    echo $array->getSize(), ':', json_encode($array->toArray()), "\n";
}

switch (getenv('RPHP_FIXED_ARRAY_CASE')) {
case 'overflow-count':
    $a = new SplFixedArray(1); $a[0] = 'retained';
    attempt(function () use ($a) { $a->fromArray([PHP_INT_MAX => 'blocked']); });
    slots($a);
    break;
case 'overflow-allocation':
    $a = new SplFixedArray(1); echo "before:", $a->getSize(), "\n";
    $a->setSize(0x2000000000000001);
    echo "unreachable\n";
    break;
case 'member-projection':
    #[AllowDynamicProperties]
    class MemberSlots extends SplFixedArray { public $label = 'member'; }
    $a = new MemberSlots(2); $a[0] = 'slot'; $a->extra = 'dynamic';
    var_dump(get_mangled_object_vars($a), (array)$a);
    attempt(function () use ($a) { new ArrayObject($a); });
    slots($a);
    break;
case 'empty-native-override':
    class NativeView extends SplFixedArray {
        public function offsetGet($i): mixed { echo "get;"; return 'override'; }
    }
    class ExistenceView extends NativeView {
        public function offsetExists($i): bool { echo "exists;"; return true; }
    }
    foreach ([new NativeView(2), new ExistenceView(2)] as $a) {
        $a[1] = 'native';
        var_dump(empty($a[0]), empty($a[1]), empty($a[8]));
        var_dump($a[0]);
        echo 'coalesce:', $a[0] ?? 'fallback', "\n";
    }
    break;
case 'construction':
    foreach ([0, 1, 4, '3', true, false, null, -1, [], new stdClass] as $size) {
        attempt(function () use ($size) { $a = new SplFixedArray($size); slots($a); });
    }
    $a = new SplFixedArray;
    slots($a);
    $a->__construct(2);
    $a[0] = 'retained';
    $a->__construct(5);
    slots($a);
    break;
case 'resize':
    $a = new SplFixedArray(3);
    $a[0] = 'left'; $a[2] = ['right'];
    var_dump($a->setSize(5)); slots($a);
    var_dump($a->setSize(1)); slots($a);
    var_dump($a->setSize(1)); slots($a);
    foreach ([-3, [], null, '4'] as $size) {
        attempt(function () use ($a, $size) { var_dump($a->setSize($size)); });
        slots($a);
    }
    $a->setSize(0); $a->setSize(2); slots($a);
    break;
case 'slots':
    $a = new SplFixedArray(3);
    $a[0] = 0; $a[1] = false;
    foreach ([0, 1, 2, -1, 3] as $key) {
        var_dump(isset($a[$key]), empty($a[$key]), $a->offsetExists($key));
    }
    unset($a[0]); slots($a);
    attempt(function () use ($a) { $a[] = 'append'; });
    slots($a);
    break;
case 'offset-coercion':
    $a = new SplFixedArray(3); $a[1] = 'one';
    foreach ([1, '1', '01', '+1', ' 1', '1.0', 1.8, true, false, null, [], new stdClass, -1, 3] as $key) {
        attempt(function () use ($a, $key) { var_dump($a->offsetGet($key)); });
        attempt(function () use ($a, $key) { var_dump($a->offsetExists($key)); });
    }
    break;
case 'offset-mutation':
    $a = new SplFixedArray(2);
    foreach (['1', '01', ' 1', 0.5, null, [], new stdClass, -2, 2] as $key) {
        attempt(function () use ($a, $key) { $a->offsetSet($key, 'set'); }); slots($a);
        attempt(function () use ($a, $key) { $a->offsetUnset($key); }); slots($a);
    }
    break;
case 'from-array':
    foreach ([[], [2 => 'two', 0 => 'zero'], ['4' => 'four'], [-1 => 'minus'], ['word' => 'value']] as $input) {
        foreach ([true, false] as $preserve) {
            attempt(function () use ($input, $preserve) { slots(SplFixedArray::fromArray($input, $preserve)); });
        }
    }
    foreach ([null, 3, new stdClass] as $input) {
        attempt(function () use ($input) { SplFixedArray::fromArray($input); });
    }
    break;
case 'projection-cow':
    $input = [['seed'], ['other']];
    $a = SplFixedArray::fromArray($input);
    $input[0][] = 'outside';
    $first = $a->toArray(); $first[1][] = 'copy';
    $a[0][] = 'inside';
    echo json_encode($input), ':', json_encode($first), "\n";
    slots($a);
    $b = clone $a; $b[1] = 'clone'; slots($a); slots($b);
    break;
case 'references':
    $seed = ['start']; $input = [&$seed, 7];
    $a = SplFixedArray::fromArray($input);
    $seed[] = 'source'; slots($a);
    $copy = $a->toArray(); $copy[0][] = 'projection'; slots($a);
    $alias =& $a[1]; $alias = 9; slots($a);
    $a[0][] = 'nested'; slots($a); echo json_encode($seed), "\n";
    $b = clone $a; $b[1] = 12; slots($a); slots($b);
    unset($a[1]); var_dump($alias); slots($a);
    break;
case 'reference-assignment':
    $a = new SplFixedArray(2); $value = 'before';
    attempt(function () use ($a, &$value) { $a[0] =& $value; });
    $value = 'after'; slots($a);
    attempt(function () use ($a) { foreach ($a as &$entry) { $entry = 'loop'; } });
    slots($a);
    break;
case 'compound':
    $a = new SplFixedArray(4); $a[0] = 3; $a[1] = 'a'; $a[2] = [];
    var_dump(++$a[0], $a[0]++, $a[0]);
    $a[1] .= 'b'; $a[2]['key'] = 7; $a[3] ??= 'fallback'; slots($a);
    unset($a[2]['key']); slots($a);
    attempt(function () use ($a) { ++$a[9]; });
    slots($a);
    break;
case 'iteration':
    $a = SplFixedArray::fromArray(['a', null, 'c']);
    foreach ($a as $key => $value) { echo $key, '=', json_encode($value), ';'; }
    echo "\n";
    $first = $a->getIterator(); $second = $a->getIterator();
    echo get_class($first), ':', (int)($first === $second), "\n";
    $first->rewind(); $first->next();
    var_dump($first->key(), $first->current(), $second->key(), $second->current());
    $a[1] = 'changed'; var_dump($first->current());
    $a->setSize(1); var_dump($first->valid(), $first->key());
    attempt(function () use ($first) { var_dump($first->current()); });
    $a->setSize(3); var_dump($first->valid(), $first->current(), $first->key());
    break;
case 'iteration-reentry':
    $a = SplFixedArray::fromArray(['first', 'second']);
    foreach ($a as $key => $value) {
        echo "$key=$value;";
        if ($key === 0) { $a->setSize(3); $a[1] = 'replaced'; $a[2] = 'grown'; }
    }
    echo "\n";
    foreach ($a as $outer => $value) {
        foreach ($a as $inner => $nested) { echo "$outer/$inner;"; break; }
    }
    echo "\n";
    break;
case 'subclass':
    class FixedChild extends SplFixedArray {
        public string $label = 'child';
        public function __construct() { echo "child-constructor;"; parent::__construct(2); }
        public function count(): int { return 44; }
    }
    $a = new FixedChild; $a[1] = 'kept';
    echo get_class($a), ':', count($a), ':', $a->getSize(), "\n";
    $b = FixedChild::fromArray(['factory']);
    echo get_class($b), ':', $b->count(), "\n";
    $c = clone $a; $c[0] = 'clone'; slots($a); slots($c);
    var_dump(get_object_vars($a), (array)$a, json_encode($a));
    break;
case 'uninitialized':
    class UnbuiltFixed extends SplFixedArray { public function __construct() {} }
    $a = new UnbuiltFixed;
    slots($a); var_dump(count($a), $a->offsetExists(0));
    attempt(function () use ($a) { var_dump($a[0]); });
    var_dump($a->setSize(1)); slots($a);
    attempt(function () use ($a) { $a[0] = 'ready'; }); slots($a);
    $a->__construct(); slots($a);
    attempt(function () {
        $b = (new ReflectionClass(SplFixedArray::class))->newInstanceWithoutConstructor();
        slots($b); $b->__construct(2); slots($b);
    });
    break;
case 'retirement':
    class FixedRetired {
        public function __construct(public $name) {}
        public function __destruct() { echo "drop:$this->name;"; }
    }
    $a = new SplFixedArray(4);
    $a[0] = new FixedRetired('zero'); $a[1] = new FixedRetired('one');
    $a[2] = new FixedRetired('two'); $a[3] = new FixedRetired('three');
    $a->setSize(1); echo "resized;";
    $a[0] = 'replacement'; echo "replaced;\n";
    break;
case 'resize-reentry':
    class FixedResizeHook {
        public function __construct(public $owner, public $name) {}
        public function __destruct() {
            echo "drop:$this->name:size=", $this->owner->getSize(), ';';
            if ($this->name === 'one') {
                var_dump($this->owner->setSize(4));
                attempt(function () { $this->owner[3] = 'reentered'; });
            }
        }
    }
    $a = new SplFixedArray(3); $a[0] = 'keep';
    $a[1] = new FixedResizeHook($a, 'one'); $a[2] = new FixedResizeHook($a, 'two');
    $a->setSize(1); echo "done;"; slots($a);
    break;
case 'resize-throw':
    class FixedThrowHook {
        public function __construct(public $name) {}
        public function __destruct() { echo "drop:$this->name;"; if ($this->name === 'first') { throw new Exception('retirement'); } }
    }
    $a = new SplFixedArray(3); $a[0] = 'kept';
    $a[1] = new FixedThrowHook('first'); $a[2] = new FixedThrowHook('second');
    attempt(function () use ($a) { $a->setSize(1); }); slots($a);
    $a->setSize(2); slots($a);
    break;
case 'throwing-coercion':
    $a = new SplFixedArray(2); $a[0] = 'kept';
    set_error_handler(function ($level, $message) { throw new Exception($message); });
    attempt(function () use ($a) { $a->setSize(null); }); slots($a);
    attempt(function () use ($a) { $a[0.5] = 'changed'; }); slots($a);
    break;
case 'debug-projection':
    $a = new SplFixedArray(2); $a[1] = 'value';
    var_dump($a);
    var_dump(get_object_vars($a), (array)$a, $a->jsonSerialize());
    break;
case 'print-projection':
    $a = new SplFixedArray(3); $a[1] = 'middle';
    echo print_r($a, true);
    $a->setSize(2); echo print_r($a, true);
    $b = new SplFixedArray(1); $b[0] = $a; $a[0] = $b;
    echo print_r($a, true);
    break;
case 'object-slot':
    $a = new SplFixedArray(2); $a[0] = (object)['number' => 3]; $a[1] = $a;
    $a[0]->number += 4; $a[1][0]->number++;
    $view =& $a[0]; echo $view->number, ':', (int)($view === $a[0]), "\n";
    $view = 'detached'; echo $a[0]->number, "\n";
    $replacement = 'blocked';
    attempt(function () use ($a, &$replacement) { $a[0] =& $replacement; });
    echo $a[0]->number, "\n";
    unset($a[0]->number); var_dump(isset($a[0]->number));
    break;
case 'exists-conversion':
    $a = new SplFixedArray(2); $a[1] = 'nonempty';
    var_dump(empty($a[1.5]), isset($a[1.5]), empty($a[0.5]), $a->offsetGet(1.5));
    break;
case 'overloaded-object-view':
    class ObjectView implements ArrayAccess {
        public $value;
        public function __construct() { $this->value = (object)['number' => 5]; }
        public function offsetGet(mixed $key): mixed { echo "get;"; return $this->value; }
        public function offsetSet(mixed $key, mixed $value): void { echo "set;"; $this->value = $value; }
        public function offsetExists(mixed $key): bool { return true; }
        public function offsetUnset(mixed $key): void {}
    }
    $view = new ObjectView;
    $view[0]->number++;
    $alias =& $view[0]; echo $alias->number, "\n";
    $alias = null; echo $view->value->number, "\n";
    $view->value = 8;
    $alias =& $view[0]; $alias = 9; echo $view->value, "\n";
    break;
case 'cursor-cycle':
    gc_collect_cycles();
    $a = new SplFixedArray(1); $a[0] = $a->getIterator();
    $weak = WeakReference::create($a); unset($a); gc_collect_cycles();
    var_dump($weak->get() === null);
    $a = new SplFixedArray(1); $a[0] = 'held'; $iterator = $a->getIterator();
    $weak = WeakReference::create($a); unset($a);
    var_dump($weak->get() !== null, $iterator->current());
    unset($iterator); gc_collect_cycles(); var_dump($weak->get() === null);
    break;
case 'overwrite-reentry':
    class FixedOverwriteHook {
        public function __construct(public $owner) {}
        public function __destruct() { echo 'observed:', json_encode($this->owner->toArray()), ';'; $this->owner[0] = 'callback'; }
    }
    $a = new SplFixedArray(1); $a[0] = new FixedOverwriteHook($a);
    $a[0] = 'published'; slots($a);
    break;
case 'cycles':
    gc_collect_cycles();
    $a = new SplFixedArray(1); $a[0] = $a;
    $weak = WeakReference::create($a); unset($a); gc_collect_cycles();
    var_dump($weak->get() === null);
    $a = new SplFixedArray(1); $a[0] = [$a];
    $weak = WeakReference::create($a); unset($a); gc_collect_cycles();
    var_dump($weak->get() === null);
    break;
case 'evaluation-order':
    $a = new SplFixedArray(1); $a[0] = 'old';
    function fixedRight($a) { echo 'right;'; $a[0] = 'side-effect'; return 'assigned'; }
    attempt(function () use ($a) { $a[3] = fixedRight($a); }); slots($a);
    attempt(function () use ($a) { $a[3] += fixedRight($a); }); slots($a);
    break;
case 'reflection':
    $r = new ReflectionClass(SplFixedArray::class);
    echo implode(',', $r->getInterfaceNames()), ':', count($r->getProperties()), "\n";
    foreach (['__construct', 'getSize', 'setSize', 'fromArray', 'offsetGet', 'getIterator', 'jsonSerialize'] as $name) {
        $m = $r->getMethod($name);
        echo $name, ':', $m->getNumberOfRequiredParameters(), '/', $m->getNumberOfParameters(), ':', (int)$m->isStatic(), ':';
        foreach ($m->getParameters() as $p) { echo $p->getName(), ','; }
        echo ':', $m->hasReturnType() ? $m->getReturnType() : ($m->hasTentativeReturnType() ? '~' . $m->getTentativeReturnType() : '-'), "\n";
    }
    break;
default:
    throw new Exception('Unknown specimen');
}
