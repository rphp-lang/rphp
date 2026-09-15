<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
function emit($value) { echo json_encode($value), "\n"; }
function attempt($name, $body) {
    echo "$name:";
    try { $body(); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
try {
switch (getenv('RPHP_ARRAY_OBJECT_PROJECTION_CASE')) {
case 'enum_admission':
    enum Colour { case Blue; }
    class View extends ArrayObject {}
    foreach (['ArrayObject', 'ArrayIterator', 'RecursiveArrayIterator', 'View'] as $class) {
        attempt($class, function () use ($class) { new $class(Colour::Blue); echo "accepted\n"; });
    }
    $view = new ArrayObject(['old' => 7]);
    attempt('exchange', fn() => $view->exchangeArray(Colour::Blue));
    emit($view->getArrayCopy());
    attempt('restore', fn() => $view->__unserialize([0, Colour::Blue, [], null]));
    emit($view->getArrayCopy());
    break;
case 'overloaded_admission':
    class IntervalChild extends DateInterval {}
    class View extends ArrayObject {}
    foreach ([new DateInterval('P2D'), new IntervalChild('P3D'), new DateTime('2001-01-01'), new DateTimeZone('UTC')] as $object) {
        $view = new View(['retained' => 4]);
        attempt(get_class($object), fn() => $view->exchangeArray($object));
        emit($view->getArrayCopy());
    }
    class MagicBacking {
        public $kept = 8;
        public function __get($name) { echo "magic-get\n"; return 90; }
        public function __debugInfo() { return ['fake' => 99]; }
    }
    $view = new ArrayObject(new MagicBacking);
    emit($view->getArrayCopy());
    break;
case 'legacy_admission':
    $view = new ArrayObject(['retained' => 8]);
    attempt('legacy', fn() => $view->unserialize('x:i:0;O:12:"DateInterval":0:{};m:a:0:{}'));
    emit($view->getArrayCopy());
    break;
case 'diagnostic_abort':
    $view = new ArrayObject(['retained' => 6]);
    $incoming = new DateInterval('P1D');
    set_error_handler(function ($level, $message) { echo "before-reject\n"; throw new LogicException('diagnostic stopped'); });
    attempt('abort', fn() => $view->exchangeArray($incoming));
    restore_error_handler(); emit($view->getArrayCopy());
    break;
case 'numeric_property_keys':
    $object = new stdClass;
    $view = new ArrayObject($object);
    $view[2] = 7; $view['02'] = 9; $view[-3] = 5;
    $view[2] += 1;
    var_dump((array) $view, $view->getArrayCopy());
    foreach ($view as $key => $value) emit([gettype($key), $key, $value]);
    emit([isset($view[2]), $object->{'2'}]);
    unset($view[2]); emit((array) $object);
    break;
case 'declared_value_sort':
    class SortedSlots { public $z = 8; private $secret = 1; protected $middle = 4; public int $typed = 2; }
    $object = new SortedSlots;
    $view = new ArrayObject($object);
    emit($view->asort()); emit((array) $object); emit($view->getArrayCopy());
    $object->typed = 12; $view['z'] = 3;
    emit((array) $object);
    foreach ($view as $key => $value) emit([$key, $value]);
    break;
case 'dynamic_key_sort':
    $object = (object) ['item12' => 2, 'item3' => 8, 'A' => 4, 'a' => 4];
    $view = new ArrayObject($object);
    emit($view->ksort(SORT_NATURAL)); emit((array) $object);
    emit($view->natsort()); emit((array) $object);
    emit($view->natcasesort()); emit((array) $object);
    $object->tail = 6; unset($object->item3); $object->item3 = 9;
    emit((array) $object);
    break;
case 'nested_and_self_sort':
    $object = (object) ['high' => 9, 'low' => 2];
    $inner = new ArrayObject($object); $outer = new ArrayObject($inner);
    emit($outer->uasort(fn($a, $b) => $a <=> $b));
    emit($inner->getArrayCopy()); emit((array) $object);
    $self = new ArrayObject; $self->exchangeArray($self);
    $self->high = 7; $self->low = 3;
    emit($self->asort()); var_dump($self); emit((array) $self);
    break;
case 'sort_references_cow':
    $scalar = 6; $array = ['kept' => 8];
    $object = new stdClass; $object->z =& $scalar; $object->a = 2; $object->tree = $array;
    $view = new ArrayObject($object); $before = (array) $object;
    emit($view->uksort(fn($a, $b) => strcmp($a, $b)));
    $scalar = 11; $object->tree['new'] = 4;
    emit([$before, (array) $object, $array, $view->getArrayCopy()]);
    break;
case 'sort_comparison_throw':
    $object = (object) ['c' => 3, 'a' => 1, 'b' => 2];
    $view = new ArrayObject($object); $calls = 0;
    attempt('sort', function () use ($view, &$calls) {
        $view->uasort(function ($a, $b) use (&$calls) {
            emit([$a, $b]);
            if (++$calls == 2) throw new LogicException('comparison');
            return $a <=> $b;
        });
    });
    emit((array) $object); $view['after'] = 4; emit($view->getArrayCopy());
    break;
case 'sort_reentry':
    $object = (object) ['b' => 7, 'a' => 3]; $view = new ArrayObject($object);
    $calls = 0;
    $view->uasort(function ($a, $b) use ($view, $object, &$calls) {
        if ($calls++ == 0) {
            attempt('guard', fn() => $view->offsetSet('blocked', 1));
            $object->external = 9;
        }
        return $a <=> $b;
    });
    emit((array) $object); emit($view->getArrayCopy());
    break;
case 'lazy_proxy_current':
    class LazyBacking { public int $x = 7; public int $y = 9; }
    $r = new ReflectionClass(LazyBacking::class);
    $object = $r->newLazyProxy(function () { echo "initialize\n"; return new LazyBacking; });
    $view = new RecursiveArrayIterator($object); echo "constructed\n";
    emit([$view->current(), $view->key()]); $view->next(); emit($view->current());
    $view->rewind(); emit($view->current());
    break;
case 'lazy_proxy_retry':
    class RetryBacking { public int $x = 4; }
    $r = new ReflectionClass(RetryBacking::class); $calls = 0;
    $object = $r->newLazyProxy(function () use (&$calls) { throw new LogicException('retry-' . ++$calls); });
    $view = new RecursiveArrayIterator($object); echo "constructed\n";
    foreach (['current', 'key', 'valid', 'next', 'rewind', 'current'] as $method) attempt($method, fn() => emit($view->$method()));
    emit($calls);
    break;
case 'lazy_projection':
    class ProjectedBacking { public int $x = 5; }
    $r = new ReflectionClass(ProjectedBacking::class);
    foreach (['getArrayCopy', 'count', 'read', 'cast', 'write', 'asort'] as $operation) {
        $object = $r->newLazyGhost(function ($o) use ($operation) { echo "initialize:$operation\n"; $o->x = 12; });
        $view = new ArrayObject($object); echo "constructed:$operation\n";
        if ($operation === 'read') emit($view['x']);
        elseif ($operation === 'cast') emit((array) $view);
        elseif ($operation === 'write') { $view['x'] = 8; emit($object->x); }
        else emit($view->$operation());
    }
    break;
case 'raw_slot_constraints':
    class RawBacking { public readonly int $locked; public int $typed = 1; private $secret = 4; public function __construct() { $this->locked = 7; } }
    $object = new RawBacking; $view = new ArrayObject($object);
    $view['typed'] = 'raw'; $view['locked'] = 6;
    emit($view->asort()); emit((array) $object); emit($view->getArrayCopy());
    break;
case 'array_backing_control':
    $array = ['z' => 8, 'a' => 3]; $view = new ArrayObject($array);
    emit($view->asort()); emit([$view->getArrayCopy(), $array]);
    $view->exchangeArray(['p' => 2]); emit($view->getArrayCopy());
    break;
case 'legacy_rejection_members':
    $view = new ArrayObject(['retained' => 5]);
    attempt('legacy', fn() => $view->unserialize('x:i:0;O:12:"DateInterval":0:{};m:a:1:{s:4:"note";s:4:"kept";}'));
    emit([$view->getArrayCopy(), $view->note]);
    $view = new ArrayObject(['retained' => 6]);
    set_error_handler(function ($level, $message) { echo "diagnostic-stopped\n"; throw new LogicException('stop'); });
    attempt('abort', fn() => $view->unserialize('x:i:0;O:12:"DateInterval":0:{};m:a:1:{s:4:"note";s:4:"kept";}'));
    restore_error_handler(); emit([$view->getArrayCopy(), isset($view->note)]);
    break;
case 'sorted_object_surfaces':
    class SurfaceBacking {
        public $last = 9; private $hidden = 1; public int $number = 3;
        public function inspect() { emit(get_object_vars($this)); foreach ($this as $k => $v) emit([$k, $v]); }
    }
    $object = new SurfaceBacking; $view = new ArrayObject($object); $view->asort();
    $object->number = 10; emit([$object->number, $view['number']]);
    emit(get_object_vars($object)); foreach ($object as $k => $v) emit([$k, $v]);
    $object->inspect(); echo serialize($object), "\n";
    $clone = clone $object; emit([$clone->number, (array) $clone]);
    break;
case 'sorted_table_retirement':
    class EntryLifetime { public function __construct(public $label) {} public function __destruct() { echo 'drop:', $this->label, "\n"; } }
    class LifetimeBacking { public $child; }
    $object = new LifetimeBacking; $object->child = new EntryLifetime('initial');
    $view = new ArrayObject($object); $view->ksort();
    $object->child = new EntryLifetime('replacement'); echo "replaced-slot\n";
    unset($view['child']); echo "removed-table\n";
    unset($view, $object); echo "done\n";
    break;
case 'lazy_foreach_throw':
    class ForeachBacking { public $value = 8; }
    $r = new ReflectionClass(ForeachBacking::class); $attempts = 0;
    $object = $r->newLazyProxy(function () use (&$attempts) { if (++$attempts < 2) throw new LogicException('first'); return new ForeachBacking; });
    $view = new ArrayIterator($object);
    attempt('first', function () use ($view) { foreach ($view as $k => $v) emit([$k, $v]); });
    attempt('retry', function () use ($view) { foreach ($view as $k => $v) emit([$k, $v]); });
    emit($attempts);
    break;
case 'native_payload_trace':
    ini_set('zend.exception_ignore_args', '0');
    set_error_handler(function ($level, $message, $file, $line) {
        emit([$level, $file === __FILE__, $line > 0]);
        foreach (array_slice(debug_backtrace(), 1, 2) as $frame) emit([$frame['class'] ?? '', $frame['function'], isset($frame['file'])]);
        return true;
    });
    $payload = 'x:i:0;O:12:"DateInterval":0:{};m:a:0:{}';
    try { unserialize('C:11:"ArrayObject":'.strlen($payload).':{'.$payload.'}'); }
    catch (Throwable $error) {
        emit([get_class($error), $error->getFile() === __FILE__, $error->getLine() > 0]);
        foreach ($error->getTrace() as $frame) emit([$frame['class'] ?? '', $frame['function'], isset($frame['file']), count($frame['args'])]);
    }
    restore_error_handler();
    break;
case 'sorted_output_views':
    class DisplayBacking { public $last = 9; protected $middle = 2; private $hidden = 1; public $number = 3; }
    $object = new DisplayBacking; $view = new ArrayObject($object); $view->asort(); $object->number = 10;
    emit($object); echo http_build_query($object), "\n"; print_r($object); echo var_export($object, true), "\n";
    reset($object);
    do { emit([key($object), current($object)]); } while (next($object) !== false);
    break;
case 'sorted_cycle_gc':
    class CyclicBacking { public $edge; public function __destruct() { echo "cycle-retired\n"; } }
    $object = new CyclicBacking; $object->edge = $object; $weak = WeakReference::create($object);
    $view = new ArrayObject($object); $view->ksort(); unset($object, $view); echo "unrooted\n";
    gc_collect_cycles(); emit($weak->get() === null);
    $object = $weak->get(); $view = new ArrayObject($object);
    $object->edge = null; unset($view['edge']); unset($object, $view);
    emit($weak->get() === null);
    break;
case 'lazy_sort_argument_order':
    class OrderedBacking { public $value = 2; }
    $r = new ReflectionClass(OrderedBacking::class);
    foreach (['asort', 'uasort'] as $method) {
        $object = $r->newLazyGhost(function ($object) { echo "initialized\n"; $object->value = 3; });
        $view = new ArrayObject($object); attempt($method, fn() => $view->$method([])); emit($r->isUninitializedLazyObject($object));
    }
    break;
case 'sorted_hook_iteration':
    class HookBacking { public $x = 7 { get { echo "getter\n"; return $this->x + 1; } } public int $virtual { get => 3; } }
    $object = new HookBacking; $view = new ArrayObject($object); $view->asort();
    emit((array) $object); emit($object); foreach ($object as $key => $value) emit([$key, $value]);
    print_r($object); echo http_build_query($object), "\n";
    break;
default: throw new LogicException('unknown specimen');
}
} catch (Throwable $e) { echo 'uncaught:', get_class($e), ':', $e->getMessage(), "\n"; }
