<?php
// Original native deque specimens, each executed in its own PHP request.
set_error_handler(function ($level, $message) {
    echo "diagnostic:$level:$message\n";
    return true;
});
function attempt($body) {
    try { $body(); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
function makeList($class = 'SplDoublyLinkedList') {
    $list = new $class;
    foreach (['a', 'b', 'c'] as $value) { $list->push($value); }
    return $list;
}
function values($list) {
    $result = [];
    for ($i = 0; $i < $list->count(); ++$i) { $result[] = $list[$i]; }
    echo json_encode($result), "\n";
}
function cursor($list) {
    echo (int)$list->valid(), ':', $list->key(), ':', json_encode($list->current()), "\n";
}
switch (getenv('RPHP_DEQUE_CASE')) {
case 'property-fetch-consumer-boundaries':
    class DequeFetchedValues {
        public int $number = 12;
        public string $text = 'abcd';
        public array $items = ['seed'];
    }
    $value = new DequeFetchedValues;
    for ($round = 0; $round < 3; ++$round) {
        var_dump($value->number + 2, strlen($value->number), strlen($value->text));
        $name = 'text'; var_dump(strlen($value->$name));
        $copy = $value->items; $copy[] = 'copy';
        echo json_encode([$value->items, $copy]), "\n";
        $alias =& $value->text; $alias = 'short'; var_dump(strlen($value->text));
        $other = new DequeFetchedValues; unset($other->number);
        foreach ([$value, $other, $value] as $receiver) {
            try { var_dump(strlen($receiver->number)); }
            catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
        }
    }
    break;
case 'scalar-call-proof-boundaries':
    function dequeLeaf($value) { return $value + 1; }
    function dequeTypedLeaf(int $value): int { return $value + 1; }
    function dequeZero() { return 7; }
    function dequeEight($a, $b, $c, $d, $e, $f, $g, $h) { return $a + $h; }
    function dequeInput($value) { echo 'input;'; return $value; }
    class DequeCallBase {
        public function leaf($value) { return $value + 2; }
        public static function fixed($value) { return $value + 3; }
    }
    class DequeCallChild extends DequeCallBase {
        public function leaf($value) { return $value + 4; }
    }
    for ($round = 0; $round < 3; ++$round) {
        $source = 6; $alias =& $source;
        var_dump(dequeZero(), dequeEight(1, 2, 3, 4, 5, 6, 7, 8));
        var_dump(dequeLeaf($source), dequeLeaf('8'), dequeLeaf(PHP_INT_MAX));
        var_dump(dequeLeaf(value: 9), dequeLeaf(...[10]));
        var_dump(dequeLeaf(dequeInput(11), dequeInput(12)));
        try { dequeLeaf(); } catch (Throwable $e) { echo get_class($e), "\n"; }
        try { dequeTypedLeaf(PHP_INT_MAX); } catch (Throwable $e) { echo get_class($e), "\n"; }
        try { dequeLeaf('not numeric'); } catch (Throwable $e) { echo get_class($e), "\n"; }
        foreach ([new DequeCallBase, new DequeCallChild] as $receiver) {
            var_dump($receiver->leaf(2), $receiver->leaf(value: 3), $receiver::fixed(4));
        }
        var_dump(DequeCallChild::fixed(5), $source, $alias);
    }
    eval('declare(strict_types=1); try { dequeTypedLeaf("7"); } catch (Throwable $e) { echo get_class($e), "\n"; }');
    break;
case 'constructor-proof-boundaries':
    class DequeIntBox {
        public int $value;
        public function __construct(int $value) { $this->value = $value; }
    }
    class DequeFloatBox {
        public float $value;
        public function __construct(float $value) { $this->value = $value; }
    }
    class DequeArrayBox {
        public array $value;
        public function __construct(array $value) { $this->value = $value; }
    }
    for ($round = 0; $round < 3; ++$round) {
        foreach ([7, '8', 'invalid'] as $value) {
            attempt(function () use ($value) { $box = new DequeIntBox($value); var_dump($box->value); });
        }
        foreach ([7.5, 8, '9.5'] as $value) { $box = new DequeFloatBox($value); var_dump($box->value); }
        $value = ['seed']; $alias =& $value; $box = new DequeArrayBox($value); $box->value[] = 'box';
        echo json_encode([$value, $alias, $box->value]), "\n";
    }
    eval('declare(strict_types=1); foreach ([5, "5", 5.5] as $value) { attempt(function () use ($value) { $box = new DequeIntBox($value); var_dump($box->value); }); } $box = new DequeFloatBox(8); var_dump($box->value);');
    break;
case 'reference-append-owning-slot':
    class DequeReferenceAccess implements ArrayAccess {
        public array $values = [];
        public function offsetExists(mixed $key): bool { return false; }
        public function offsetGet(mixed $key): mixed { echo "get\n"; return null; }
        public function offsetSet(mixed $key, mixed $value): void { echo "set\n"; $this->values[] = $value; }
        public function offsetUnset(mixed $key): void {}
    }
    $source = 'before'; $alias =& $source;
    foreach ([new DequeReferenceAccess, new ArrayObject, new SplDoublyLinkedList] as $a) {
        attempt(function () use ($a, &$source) { $a[] =& $source; });
        $source = 'after'; echo $source, ':', $alias, "\n";
    }
    break;
case 'reference-append-getter-contract':
    class DequeTransient {
        public function __construct(public bool $throw = false) {}
        public function __destruct() { echo "retired;"; if ($this->throw) { throw new Exception('retirement stopped'); } }
    }
    class DequeAppendGetter implements ArrayAccess {
        public $mode;
        public function offsetExists(mixed $key): bool { return true; }
        public function offsetGet(mixed $key): mixed {
            echo "get;";
            if ($this->mode === 'throw') { throw new Exception('getter stopped'); }
            return new DequeTransient($this->mode === 'retirement-throw');
        }
        public function offsetSet(mixed $key, mixed $value): void { echo "unexpected-set;"; }
        public function offsetUnset(mixed $key): void {}
    }
    class DequeAppendRefGetter extends DequeAppendGetter {
        public $slot = 'old';
        public function &offsetGet(mixed $key): mixed { echo "ref-get;"; return $this->slot; }
    }
    $source = 'new';
    foreach ([new DequeAppendGetter, new DequeAppendRefGetter] as $a) {
        attempt(function () use ($a, &$source) { $a[] =& $source; });
    }
    $a = new DequeAppendGetter; $a->mode = 'throw';
    attempt(function () use ($a, &$source) { $a[] =& $source; });
    $a->mode = 'retirement-throw';
    try { $a[] =& $source; } catch (Throwable $e) {
        echo $e->getMessage(), ':', $e->getPrevious()?->getMessage(), "\n";
    }
    echo $source, "\n";
    break;
case 'delete-mode-transition':
    foreach ([0, 2] as $mode) {
        $a = makeList(); $a->push('d'); $a->setIteratorMode($mode); $a->rewind(); $a->next();
        $a->setIteratorMode($mode | 1); $a->next(); cursor($a); values($a);
        $a->prev(); cursor($a); values($a);
    }
    break;
case 'retired-cursor-slot-reuse':
    $a = makeList(); $a->rewind(); $a->shift(); $a->unshift('new');
    cursor($a); $a->next(); cursor($a); values($a);
    $a->rewind(); $a->next(); unset($a[1]); $a->add(1, 'replacement');
    cursor($a); $a->next(); cursor($a); values($a);
    break;
case 'consumer-projections':
    $a = makeList(); $a->rewind(); $a->next();
    echo json_encode(iterator_to_array($a)), ':', iterator_count($a), "\n"; cursor($a);
    $it = new IteratorIterator($a); $it->rewind(); $it->next(); cursor($a); var_dump($it->getInnerIterator() === $a, $it->current());
    echo json_encode([...$a]), "\n"; cursor($a);
    $gen = (function () use ($a) { yield from $a; })(); echo json_encode(iterator_to_array($gen)), "\n"; cursor($a);
    break;
case 'invisible-cursor':
    $a = makeList(); $objects = [];
    foreach ($a as $value) { $object = new stdClass; $objects[] = $object; echo spl_object_id($object), ';'; }
    echo "\n"; unset($objects, $object); $object = new stdClass; echo spl_object_id($object), "\n";
    break;
case 'prev-delete':
    foreach ([0, 1, 2, 3] as $mode) {
        $a = makeList(); $a->setIteratorMode($mode); $a->rewind(); $a->next(); $a->prev(); cursor($a); values($a);
    }
    break;
case 'empty-coercion':
    $a = new SplDoublyLinkedList; $a->push(null); $a->push(0); $a->push('nonempty');
    foreach ([0, 1, 2, 4, '01', 1.5, null] as $key) {
        attempt(function () use ($a, $key) { var_dump(isset($a[$key]), empty($a[$key]), $a[$key] ?? 'fallback'); });
    }
    break;
case 'append-contexts':
    $a = new SplDoublyLinkedList; $a->push('kept');
    attempt(function () use ($a) { ++$a[]; });
    attempt(function () use ($a) { $a[] += 2; });
    $source = 'alias'; attempt(function () use ($a, &$source) { $a[] =& $source; }); values($a);
    break;
case 'strict-indices':
    eval('declare(strict_types=1); $a = new SplDoublyLinkedList; $a->push("kept"); foreach (["0", 0.0, true, null] as $key) { attempt(function () use ($a,$key) { var_dump($a->offsetGet($key)); }); attempt(function () use ($a,$key) { $a->add($key,"blocked"); }); } values($a);');
    break;
case 'delete-reentry':
    class DequeDeleteHook {
        public function __construct(public $owner) {}
        public function __destruct() { echo 'hook:', $this->owner->count(), ';'; $this->owner->push('appended'); }
    }
    $a = new SplDoublyLinkedList; $a->push(new DequeDeleteHook($a)); $a->push('last');
    $a->setIteratorMode(SplDoublyLinkedList::IT_MODE_DELETE); $step = 0;
    foreach ($a as $key => $value) { echo $key, ':', is_object($value) ? 'object' : $value, ';'; if ($step++ > 4) { break; } }
    echo "\n"; values($a);
    break;
case 'ends':
    foreach (['SplDoublyLinkedList', 'SplStack', 'SplQueue'] as $class) {
        $a = new $class; echo $class, ':', $a->getIteratorMode(), ':', (int)$a->isEmpty(), "\n";
        var_dump($a->push('right'), $a->unshift('left')); $a[] = 'last'; values($a);
        var_dump($a->bottom(), $a->top(), $a->pop(), $a->shift()); values($a);
    }
    $q = new SplQueue; var_dump($q->enqueue('first'), $q->enqueue('second'), $q->dequeue()); values($q);
    break;
case 'empty':
    $a = new SplDoublyLinkedList; cursor($a); $a->next(); cursor($a); $a->prev(); cursor($a); $a->rewind(); cursor($a);
    foreach (['top', 'bottom', 'pop', 'shift'] as $method) { attempt(function () use ($a, $method) { $a->$method(); }); }
    foreach ([0, -1, null, '0', 'abc', 0.5, [], new stdClass] as $key) {
        attempt(function () use ($a, $key) { var_dump($a->offsetExists($key)); });
        attempt(function () use ($a, $key) { var_dump($a->offsetGet($key)); });
        attempt(function () use ($a, $key) { $a->offsetUnset($key); });
    }
    break;
case 'offsets':
    foreach ([0, 2] as $mode) {
        $a = makeList(); $a->setIteratorMode($mode); values($a);
        foreach ([0, 1, 3, -1, '1', '01', '1.5', 'x', true, null, 1.9, []] as $key) {
            attempt(function () use ($a, $key) { var_dump($a->offsetExists($key), $a->offsetGet($key)); });
        }
        $a[0] = 'zero'; unset($a[1]); $a->offsetSet(null, 'appended'); values($a);
    }
    break;
case 'add':
    foreach ([0, 2] as $mode) {
        foreach ([0, 1, 3, 4, -1, null, '1', 1.9, []] as $position) {
            $a = makeList(); $a->setIteratorMode($mode);
            attempt(function () use ($a, $position) { var_dump($a->add($position, 'insert')); }); values($a);
        }
    }
    break;
case 'modes':
    foreach (['SplDoublyLinkedList', 'SplStack', 'SplQueue'] as $class) {
        echo $class, "\n"; $a = makeList($class);
        foreach ([0, 1, 2, 3, 4, 5, 6, 7, -1, null, '2', []] as $mode) {
            attempt(function () use ($a, $mode) { var_dump($a->setIteratorMode($mode)); });
            echo 'mode:', $a->getIteratorMode(), ';'; values($a);
        }
    }
    break;
case 'manual-cursor':
    foreach ([0, 1, 2, 3] as $mode) {
        echo "mode:$mode\n"; $a = makeList(); $a->setIteratorMode($mode); cursor($a); $a->rewind(); cursor($a);
        for ($i = 0; $i < 4; ++$i) { $a->next(); cursor($a); }
        values($a); $a->prev(); cursor($a); $a->rewind(); cursor($a);
    }
    break;
case 'cursor-mutations':
    foreach (['shift', 'pop', 'unset-current', 'unset-before', 'add-before', 'unshift', 'push', 'replace'] as $action) {
        echo "$action\n"; $a = makeList(); $a->rewind(); $a->next(); cursor($a);
        switch ($action) {
        case 'shift': $a->shift(); break;
        case 'pop': $a->pop(); break;
        case 'unset-current': unset($a[1]); break;
        case 'unset-before': unset($a[0]); break;
        case 'add-before': $a->add(1, 'x'); break;
        case 'unshift': $a->unshift('x'); break;
        case 'push': $a->push('x'); break;
        case 'replace': $a[1] = 'x'; break;
        }
        cursor($a); $a->next(); cursor($a); $a->prev(); cursor($a); values($a);
    }
    break;
case 'foreach':
    foreach ([0, 1, 2, 3] as $mode) {
        echo "mode:$mode\n"; $a = makeList(); $a->setIteratorMode($mode); $a->rewind(); $a->next(); cursor($a);
        foreach ($a as $key => $value) { echo "$key=$value;"; } echo "\n"; cursor($a); values($a);
    }
    $a = makeList(); foreach ($a as $outer => $value) { foreach ($a as $inner => $nested) { echo "$outer/$inner;"; break; } } echo "\n";
    attempt(function () use ($a) { foreach ($a as &$value) { $value = 'ref'; } }); values($a);
    break;
case 'foreach-mutation':
    foreach (['unshift', 'add-before', 'unset-current', 'shift', 'replace', 'push'] as $action) {
        echo "$action\n"; $a = makeList(); $step = 0;
        foreach ($a as $key => $value) {
            echo "$key=$value;";
            if ($step++ === 1) {
                switch ($action) {
                case 'unshift': $a->unshift('x'); break;
                case 'add-before': $a->add(1, 'x'); break;
                case 'unset-current': unset($a[1]); break;
                case 'shift': $a->shift(); break;
                case 'replace': $a[1] = 'x'; break;
                case 'push': $a->push('x'); break;
                }
            }
            if ($step > 6) { break; }
        }
        echo "\n"; values($a);
    }
    break;
case 'mode-during-iteration':
    $a = makeList(); $a->rewind(); $a->next(); $a->setIteratorMode(2); cursor($a); $a->next(); cursor($a);
    $a = makeList(); $step = 0;
    foreach ($a as $key => $value) { echo "$key=$value;"; if ($step++ === 0) { $a->setIteratorMode(2); } if ($step > 4) { break; } }
    echo "\n"; values($a);
    break;
case 'clone':
    $a = makeList('SplStack'); $a->rewind(); $a->next(); $b = clone $a;
    cursor($a); cursor($b); $b->push('clone'); $a->pop(); values($a); values($b); cursor($a); cursor($b);
    $source = ['seed']; $a = new SplDoublyLinkedList; $a->push($source); $b = clone $a; $source[] = 'outside';
    $b[0][] = 'nested'; values($a); values($b);
    break;
case 'references':
    $a = new SplDoublyLinkedList; $source = ['seed']; $a->push($source); $source[] = 'source';
    $alias =& $a[0]; $alias[] = 'alias'; $a[0][] = 'nested'; values($a); echo json_encode($source), "\n";
    attempt(function () use ($a, &$source) { $a[0] =& $source; }); values($a);
    $object = (object)['n' => 4]; $a[0] = $object; $a[0]->n++; $alias =& $a[0];
    echo $object->n, ':', (int)($alias === $object), "\n"; $alias = null; echo $a[0]->n, "\n";
    break;
case 'compound':
    $a = new SplDoublyLinkedList; $a->push(3); $a->push('a');
    var_dump(++$a[0], $a[0]++, $a[0]); $a[1] .= 'z'; values($a);
    attempt(function () use ($a) { ++$a[9]; }); attempt(function () use ($a) { $a[9] += 2; });
    $a[] = 'append'; values($a);
    break;
case 'retirement':
    class DequeRetired {
        public function __construct(public $name) {}
        public function __destruct() { echo "drop:$this->name;"; }
    }
    $a = new SplDoublyLinkedList; foreach (['left', 'middle', 'right'] as $name) { $a->push(new DequeRetired($name)); }
    $a->rewind(); $a->next(); unset($a[1]); echo 'removed;'; cursor($a); $a->next(); echo 'advanced;';
    unset($a); echo "released\n";
    break;
case 'reentry':
    class DequeReplace {
        public function __construct(public $owner) {}
        public function __destruct() { echo 'observed:', $this->owner->count(), ':', json_encode($this->owner[0]), ';'; $this->owner->push('hook'); }
    }
    $a = new SplDoublyLinkedList; $a->push(new DequeReplace($a)); $a[0] = 'new'; values($a);
    break;
case 'cycles':
    gc_collect_cycles(); $a = new SplDoublyLinkedList; $a->push($a); $weak = WeakReference::create($a);
    unset($a); gc_collect_cycles(); var_dump($weak->get() === null);
    $a = makeList(); $weak = WeakReference::create($a); $it = new IteratorIterator($a); unset($a);
    var_dump($weak->get() !== null); unset($it); gc_collect_cycles(); var_dump($weak->get() === null);
    break;
case 'projection':
    $a = makeList(); var_dump($a, (array)$a, $a->__debugInfo(), get_object_vars($a), json_encode($a));
    $s = makeList('SplStack'); var_dump($s);
    break;
case 'subclass':
    class CountedDeque extends SplDoublyLinkedList {
        public string $label = 'member';
        public function count(): int { echo 'count-hook;'; return 45; }
        public function current(): mixed { echo 'current-hook;'; return parent::current(); }
        public function rewind(): void { echo 'rewind-hook;'; parent::rewind(); }
    }
    $a = makeList('CountedDeque'); var_dump(count($a), $a->isEmpty());
    foreach ($a as $key => $value) { echo "$key=$value;"; } echo "\n";
    var_dump((array)$a); $b = clone $a; echo $b->label, "\n";
    break;
case 'call-type-scope-boundaries':
    trait DequeScopeMethods {
        public function lexical(self &$value): self { return $value; }
        public static function number(int &$value): int { return ++$value; }
        public static function concrete(DequeScopeBase &$value): DequeScopeBase { return $value; }
        public static function named(string ...$values): array { return $values; }
        public static function generate(int &$value): Traversable { yield ++$value; }
    }
    class DequeScopeBase { use DequeScopeMethods; }
    class DequeScopeChild extends DequeScopeBase {
        public static function ancestor(parent &$value): parent { return $value; }
        public static function union(self|int &$value): self|int { return $value; }
    }
    $base = new DequeScopeBase; $child = new DequeScopeChild;
    var_dump($base->lexical($base) === $base, DequeScopeChild::ancestor($base) === $base);
    var_dump(DequeScopeChild::concrete($base) === $base, DequeScopeChild::union($child) === $child);
    $number = '6'; var_dump(DequeScopeChild::number($number), $number);
    var_dump(DequeScopeChild::named(first: 12, second: 'word'));
    foreach (DequeScopeChild::generate($number) as $value) { var_dump($value, $number); }
    $closure = function (parent $value): parent { return $value; };
    $bound = $closure->bindTo(null, DequeScopeChild::class);
    var_dump($bound($base) === $base);
    $wrong = new stdClass;
    try { DequeScopeChild::ancestor($wrong); }
    catch (TypeError $error) { echo 'parent-rejected:', get_class($wrong), "\n"; }
    try { eval('declare(strict_types=1); $number = "8"; DequeScopeChild::number($number);'); }
    catch (TypeError $error) { echo 'strict-rejected:'; var_dump($number); }
    break;
case 'property-result-slot-boundaries':
    class DequeResultSlots {
        public $payload = [];
        public string $text = 'letters';
        public int $number = 4;
    }
    class DequeRetiredResult {
        public function __destruct() { echo "retired;"; }
    }
    function dequeConsumeResult($value) { return $value; }
    $receiver = new DequeResultSlots;
    $slot = null; $alias =& $slot;
    for ($round = 0; $round < 3; ++$round) {
        $receiver->payload = ['round' => $round];
        $slot = $receiver->payload; $slot['copy'] = true;
        echo json_encode([$receiver->payload, $slot, $alias]), "\n";
        $slot = new DequeRetiredResult;
        $slot = $receiver->number;
        var_dump($alias, $receiver->number + $round);
        $slot = $receiver->text; $slot .= '!';
        var_dump($alias, strlen($receiver->text), dequeConsumeResult($receiver->payload));
        $textAlias =& $receiver->text; $textAlias = 'alias';
        var_dump(strlen($receiver->text), dequeConsumeResult($receiver->text));
    }
    break;
case 'arithmetic-projection-boundaries':
    function dequeSum($left, $right) { return $left + $right; }
    foreach ([0, 7, PHP_INT_MIN, PHP_INT_MAX, true, false, null, '42', '4.5', '4e1'] as $left) {
        $alias =& $left;
        var_dump(dequeSum($alias, 1));
        var_dump(dequeSum(1, $alias));
    }
    foreach (['not numeric', new stdClass] as $invalid) {
        try { dequeSum($invalid, 1); }
        catch (TypeError $error) { echo "rejected;"; }
    }
    $resource = fopen('php://memory', 'w+');
    try { dequeSum($resource, 1); }
    catch (TypeError $error) { echo "resource-rejected\n"; }
    fclose($resource);
    echo json_encode(dequeSum(['left' => 1], ['right' => 2])), "\n";
    break;
default: throw new Exception('Unknown deque specimen');
}
