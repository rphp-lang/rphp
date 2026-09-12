<?php
function observe($label, $action) {
    echo $label, ':';
    try { echo json_encode($action(), JSON_UNESCAPED_SLASHES), "\n"; }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
function drain($heap) {
    $values = [];
    while (!$heap->isEmpty()) $values[] = $heap->extract();
    return $values;
}
if (class_exists('SplHeap')) { class RankedHeap extends SplHeap {
    public function compare($left, $right): int { return $left['rank'] <=> $right['rank']; }
} }
if (class_exists('SplMaxHeap')) { class LoggedHeap extends SplMaxHeap {
    public bool $armed = false;
    public array $log = [];
    public function compare($left, $right): int {
        $this->log[] = [$left, $right, $this->count()];
        if ($this->armed) throw new RuntimeException('comparison-stop');
        return parent::compare($left, $right);
    }
} }
class HeapPayload {
    public static array $retired = [];
    public string $label;
    public function __construct($label) { $this->label = $label; }
    public function __destruct() { self::$retired[] = $this->label; }
}

switch (getenv('RPHP_HEAP_CASE')) {
case 'empty':
    foreach ([new SplMaxHeap, new SplMinHeap, new SplPriorityQueue] as $h) {
        echo get_class($h), "\n";
        foreach (['count', 'isEmpty', 'isCorrupted', 'valid', 'key', 'current', 'top', 'extract', 'next', 'rewind', 'recoverFromCorruption'] as $method) {
            observe($method, fn() => $h->$method());
        }
    }
    break;
case 'order':
    foreach ([new SplMaxHeap, new SplMinHeap] as $h) {
        foreach ([13, -4, 0, 13, 7, 2] as $value) $h->insert($value);
        observe('top', fn() => [$h->top(), $h->count()]);
        observe('drain', fn() => drain($h));
    }
    $h = new RankedHeap;
    foreach ([['rank'=>2,'id'=>'a'], ['rank'=>8,'id'=>'b'], ['rank'=>5,'id'=>'c']] as $value) $h->insert($value);
    observe('custom', fn() => drain($h));
    break;
case 'ties':
    $h = new SplPriorityQueue;
    foreach (['one', 'two', 'three', 'four', 'five', 'six'] as $value) $h->insert($value, 7);
    observe('ties', fn() => drain($h));
    break;
case 'flags':
    $h = new SplPriorityQueue;
    $h->insert('lower', 2); $h->insert('higher', 9);
    foreach ([-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 15] as $flags) {
        observe('set-'.$flags, fn() => $h->setExtractFlags($flags));
        observe('state', fn() => [$h->getExtractFlags(), $h->top(), $h->current(), $h->count()]);
    }
    break;
case 'compare':
    $h = new LoggedHeap;
    foreach ([5, 1, 9, 3, 8, 2] as $v) $h->insert($v);
    observe('inserts', fn() => $h->log);
    $h->log = [];
    observe('drain', fn() => drain($h));
    observe('extracts', fn() => $h->log);
    observe('priority-compare', fn() => [(new SplPriorityQueue)->compare(2, 3), (new SplPriorityQueue)->compare('10', 2)]);
    break;
case 'corrupt-insert':
    $h = new LoggedHeap; $h->insert(11); $h->armed = true;
    observe('insert', fn() => $h->insert(20));
    foreach (['count', 'isEmpty', 'isCorrupted', 'valid', 'key', 'current', 'top', 'extract', 'next', 'rewind'] as $method) observe($method, fn() => $h->$method());
    observe('again', fn() => $h->insert(6));
    observe('recovery', fn() => $h->recoverFromCorruption());
    $h->armed = false;
    observe('drain', fn() => drain($h));
    break;
case 'corrupt-extract':
    $h = new LoggedHeap;
    foreach ([12, 3, 8, 5, 1] as $v) $h->insert($v);
    $h->armed = true;
    observe('extract', fn() => $h->extract());
    observe('flags', fn() => [$h->count(), $h->isCorrupted(), $h->key(), $h->valid()]);
    $h->armed = false;
    observe('recover', fn() => $h->recoverFromCorruption());
    observe('drain', fn() => drain($h));
    break;
case 'write-lock':
    foreach (['insert', 'extract', 'next', 'recover'] as $operation) {
        $h = new class extends SplPriorityQueue {
            public string $operation = '';
            public bool $entered = false;
            public function compare($left, $right): int {
                if (!$this->entered) {
                    $this->entered = true;
                    if ($this->operation === 'insert') $this->insert('nested', 100);
                    elseif ($this->operation === 'extract') $this->extract();
                    elseif ($this->operation === 'next') $this->next();
                    else $this->recoverFromCorruption();
                }
                return parent::compare($left, $right);
            }
        };
        $h->operation = $operation; $h->insert('first', 8);
        observe($operation, fn() => $h->insert('second', 3));
        observe('state', fn() => [$h->count(), $h->isCorrupted()]);
        observe('recover', fn() => $h->recoverFromCorruption());
        observe('drain', fn() => drain($h));
    }
    break;
case 'read-lock':
    $h = new class extends SplMaxHeap {
        public bool $entered = false;
        public function compare($left, $right): int {
            if (!$this->entered) {
                $this->entered = true;
                foreach (['count', 'isEmpty', 'isCorrupted', 'valid', 'key', 'current', 'top', 'rewind'] as $method) observe($method, fn() => $this->$method());
                observe('copy', fn() => (clone $this)->count());
            }
            return parent::compare($left, $right);
        }
    };
    $h->insert(4); $h->insert(7);
    observe('drain', fn() => drain($h));
    break;
case 'iteration':
    $h = new SplMaxHeap;
    foreach ([2, 9, 4] as $v) $h->insert($v);
    foreach ($h as $k=>$v) {
        observe('visit', fn() => [$k, $v, $h->count(), $h->current()]);
        if ($v === 9) $h->insert(6);
    }
    observe('after', fn() => [$h->count(), $h->valid(), $h->key(), $h->current()]);
    break;
case 'nested-iteration':
    $h = new SplMinHeap;
    foreach ([7, 2, 4] as $v) $h->insert($v);
    foreach ($h as $k=>$v) {
        echo 'outer:', $k, ':', $v, "\n";
        foreach ($h as $inner=>$item) { echo 'inner:', $inner, ':', $item, "\n"; break; }
    }
    observe('after', fn() => $h->count());
    break;
case 'rewind':
    $h = new SplMaxHeap; $h->insert(3); $h->insert(5); $h->insert(1);
    $h->next(); $h->rewind();
    observe('state', fn() => [$h->count(), $h->key(), $h->current(), $h->top()]);
    observe('drain', fn() => drain($h));
    break;
case 'references-clone':
    $h = new SplPriorityQueue;
    $data = ['tag'=>'before']; $alias =& $data; $priority = 4;
    $h->insert($alias, $priority);
    $data['tag'] = 'after'; $priority = 99;
    $h->setExtractFlags(SplPriorityQueue::EXTR_BOTH);
    $copy = clone $h;
    $view = $h->top(); $view['data']['tag'] = 'view';
    $copy->insert('other', 8);
    observe('original', fn() => drain($h));
    observe('clone', fn() => drain($copy));
    break;
case 'object-identity':
    $h = new SplPriorityQueue;
    $payload = new stdClass; $payload->tag = 'first';
    $h->insert($payload, 1); $copy = clone $h;
    $payload->tag = 'second';
    observe('identity', fn() => [$h->top() === $payload, $copy->top() === $payload]);
    observe('state', fn() => [$h->extract()->tag, $copy->extract()->tag]);
    break;
case 'retirement':
    $h = new SplPriorityQueue;
    foreach (['first', 'second', 'third'] as $i=>$name) $h->insert(new HeapPayload($name), $i);
    $copy = clone $h; $h->next();
    observe('retained', fn() => HeapPayload::$retired);
    unset($copy); observe('copy-dropped', fn() => HeapPayload::$retired);
    unset($h); observe('all-dropped', fn() => HeapPayload::$retired);
    break;
case 'reentrant-retirement':
    $h = new SplPriorityQueue;
    $data = new class($h) {
        public $owner;
        public function __construct($owner) { $this->owner = $owner; }
        public function __destruct() { observe('destructor-count', fn() => $this->owner->count()); $this->owner->insert('replacement', 12); }
    };
    $h->insert($data, 9); unset($data);
    observe('next', fn() => $h->next());
    observe('state', fn() => [$h->count(), $h->isCorrupted()]);
    $h->recoverFromCorruption();
    observe('drain', fn() => drain($h));
    break;
case 'subclass-iterator':
    $h = new class extends SplMaxHeap {
        public function rewind(): void { echo "rewind\n"; parent::rewind(); }
        public function current(): mixed { return parent::current() * 10; }
        public function key(): int { return parent::key() + 20; }
        public function next(): void { echo "next\n"; parent::next(); }
    };
    $h->insert(6); $h->insert(2);
    foreach ($h as $k=>$v) echo $k, ':', $v, "\n";
    observe('after', fn() => $h->count());
    break;
case 'metadata':
    foreach (['SplHeap', 'SplMaxHeap', 'SplMinHeap', 'SplPriorityQueue'] as $name) {
        $class = new ReflectionClass($name);
        $parent = $class->getParentClass();
        observe($name, fn() => [$class->isAbstract(), $parent ? $parent->getName() : null]);
        foreach (['insert', 'extract', 'top', 'compare', 'recoverFromCorruption', 'count', 'key', 'next'] as $method) {
            $m = new ReflectionMethod($name, $method);
            $names = [];
            foreach ($m->getParameters() as $p) $names[] = $p->getName();
            observe($method, fn() => [$m->getDeclaringClass()->getName(), $m->isPublic(), $m->isProtected(), $m->isAbstract(), $m->isFinal(), $m->getNumberOfRequiredParameters(), $names, (string)$m->getReturnType(), (string)$m->getTentativeReturnType()]);
        }
    }
    break;
case 'named-arity':
    $h = new SplPriorityQueue;
    observe('insert', fn() => $h->insert(priority: 6, value: 'named'));
    observe('flags', fn() => $h->setExtractFlags(flags: 3));
    observe('extract', fn() => $h->extract());
    observe('missing', fn() => $h->insert('one'));
    observe('surplus', fn() => $h->top(1));
    observe('bad-name', fn() => $h->insert(data: 2, priority: 1));
    break;
case 'debug-info':
    $h = new SplMaxHeap; $h->insert(4); $h->insert(9); $h->insert(1);
    observe('heap', fn() => $h->__debugInfo());
    $q = new SplPriorityQueue; $q->insert('left', 2); $q->insert('right', 7);
    observe('queue', fn() => $q->__debugInfo());
    break;
case 'cycles':
    $h = new SplPriorityQueue;
    $payload = new stdClass; $payload->owner = $h;
    $h->insert($payload, 4);
    $owner = WeakReference::create($h); $child = WeakReference::create($payload);
    unset($h, $payload); gc_collect_cycles();
    observe('released', fn() => [$owner->get() === null, $child->get() === null]);
    break;
case 'clone-corrupt':
    $h = new LoggedHeap; $h->insert(4); $h->armed = true;
    observe('insert', fn() => $h->insert(8));
    $copy = clone $h;
    observe('corrupt-copy', fn() => [$h->isCorrupted(), $copy->isCorrupted(), $copy->count()]);
    $copy->recoverFromCorruption(); $copy->armed = false;
    observe('copy', fn() => drain($copy));
    observe('original', fn() => [$h->isCorrupted(), $h->count()]);
    break;
case 'conversion-lock':
    $h = new SplMinHeap;
    foreach (['30', '20', '10', '5'] as $v) $h->insert($v);
    $object = new class($h) {
        function __construct(public $owner) {}
        function __toString(): string {
            observe('locked', function () { $this->owner->next(); });
            observe('view', fn() => [$this->owner->count(), $this->owner->current()]);
            return '7';
        }
    };
    observe('insert', fn() => $h->insert($object));
    observe('state', fn() => [$h->count(), $h->isCorrupted(), $h->top()]);
    break;
case 'conversion-failure':
    $h = new SplMaxHeap; $h->insert('22');
    $object = new class {
        function __toString(): string { throw new LogicException('string-stop'); }
    };
    observe('insert', fn() => $h->insert($object));
    observe('state', fn() => [$h->count(), $h->isCorrupted(), $h->current()]);
    observe('recover', fn() => $h->recoverFromCorruption());
    observe('first', fn() => $h->extract());
    observe('identity', fn() => $h->extract() === $object);
    break;
case 'compare-visibility':
    foreach ([new SplMaxHeap, new SplMinHeap] as $h) {
        observe('external', fn() => $h->compare(4, 9));
        observe('callback', fn() => call_user_func([$h, 'compare'], 4, 9));
    }
    $h = new class extends SplMinHeap {
        public function exposed($a, $b) { return parent::compare($a, $b); }
    };
    observe('parent', fn() => $h->exposed(4, 9));
    break;
case 'nested-retirement':
    foreach ([false, true] as $nested) {
        $h = new SplPriorityQueue;
        $o = new class($h, $nested) {
            function __construct(public $owner, public $nested) {}
            function __destruct() {
                $v = $this->owner->current();
                observe('visible', fn() => [$this->owner->count(), ($this->nested ? $v[0] : $v) === $this]);
                observe('locked', fn() => $this->owner->insert('replacement', 17));
            }
        };
        $h->insert($nested ? [$o] : $o, 3); unset($o);
        observe('next', fn() => $h->next());
        observe('after', fn() => [$h->count(), $h->isCorrupted()]);
    }
    break;
case 'retirement-resurrection':
    $h = new SplPriorityQueue;
    $o = new class($h) {
        public static $saved;
        function __construct(public $owner) {}
        function __destruct() {
            self::$saved = $this->owner->current();
            observe('resurrect', fn() => self::$saved === $this);
        }
    };
    $class = get_class($o);
    $h->insert($o, 1); unset($o);
    $h->next();
    observe('retained', fn() => [$h->count(), $class::$saved->owner === $h]);
    $class::$saved = null;
    observe('done', fn() => $h->isEmpty());
    break;
case 'mutation-views':
    $h = new class extends SplMaxHeap {
        public bool $record = false;
        public function compare($a, $b): int {
            if ($this->record) {
                $state = $this->__debugInfo();
                observe('during', fn() => [$a, $b, $this->count(), $state["\0SplHeap\0heap"]]);
            }
            return parent::compare($a, $b);
        }
    };
    foreach ([4, 10, 6, 1] as $v) $h->insert($v);
    $h->record = true;
    $h->insert(15);
    observe('first', fn() => $h->extract());
    observe('second', fn() => $h->extract());
    break;
default: throw new RuntimeException('Unknown heap specimen');
}
