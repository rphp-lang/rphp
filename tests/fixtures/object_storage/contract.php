<?php
function report($label, $action) {
    try { echo $label, '=', serialize($action()), "\n"; }
    catch (Throwable $error) { echo $label, '!', get_class($error), ':', $error->getMessage(), "\n"; }
}
function item($name) { $object = new stdClass; $object->name = $name; return $object; }
function state($storage) {
    return [$storage->count(), $storage->key(), $storage->valid(),
        $storage->valid() ? $storage->current()->name : '-', $storage->getInfo()];
}
function view($storage) {
    $result = [];
    foreach ($storage as $key => $object) $result[] = [$key, $object->name, $storage->getInfo()];
    return $result;
}
set_error_handler(function ($level, $message) { echo "diag:$level:$message\n"; return true; });
switch (getenv('RPHP_OBJECT_STORAGE_CASE')) {
case 'empty':
    $s = new SplObjectStorage;
    foreach (['count', 'key', 'valid', 'current', 'getInfo', 'next', 'rewind'] as $method)
        report($method, fn() => $s->$method());
    report('set-empty', fn() => $s->setInfo('ignored'));
    report('seek-empty', fn() => $s->seek(0));
    report('after', fn() => state($s));
    break;
case 'identity':
    $s = new SplObjectStorage; $a = item('a'); $b = clone $a;
    $s[$a] = null; $s[$b] = false;
    report('membership', fn() => [$s->count(), isset($s[$a]), empty($s[$a]), isset($s[$b]), empty($s[$b])]);
    $s[$a] = ['new']; report('values', fn() => [$s[$a], $s[$b]]);
    report('hash', fn() => [$s->getHash($a) === spl_object_hash($a), $s->getHash($a) !== $s->getHash($b)]);
    unset($s[$a]); report('remaining', fn() => [$s->count(), $s[$b]]);
    report('missing', fn() => $s[$a]);
    break;
case 'comparison':
    $s = new SplObjectStorage; $other = new SplObjectStorage; $a = item('a'); $b = item('b');
    report('empty', fn() => [$s == $other, $s <=> $other]);
    $s[$a] = 7; report('count', fn() => [$s == $other, $s <=> $other, $other <=> $s]);
    $other[$a] = '7'; report('loose-info', fn() => [$s == $other, $s <=> $other]);
    $other[$a] = 8; report('info', fn() => [$s == $other, $s <=> $other]);
    $other[$a] = 7; $s[$b] = 9; $other[$b] = 9; $other->next();
    report('cursor-ignored', fn() => [$s == $other, $s <=> $other]);
    $s->label = 'left'; $other->label = 'right'; report('members-ignored', fn() => $s == $other);
    unset($other[$a]); $other[$a] = 7; report('order-ignored', fn() => $s == $other);
    break;
case 'closure-keys':
    $s = new SplObjectStorage; $key = fn() => 7; $other = fn() => 7;
    $s[$key] = ['value' => 3]; $s[$other] = null;
    report('identities', fn() => [$s->count(), $s->current() === $key, isset($s[$other]),
        $s->getHash($key) === spl_object_hash($key), $s->getHash($key) !== $s->getHash($other)]);
    $copy = clone $s; $info = $copy[$key]; $info['value'] = 9;
    report('cow', fn() => [$s[$key], $copy[$key]]);
    $restored = new SplObjectStorage; $data = [[&$key, 8], []];
    report('reference-key', fn() => $restored->__unserialize($data));
    $restored->__unserialize([[$other, 8], []]);
    report('restored', fn() => [$restored->count(), $restored[$other]]);
    unset($s[$key]); report('removed', fn() => [$s->count(), $s->current() === $other]);
    report('wire-rejects-closure', fn() => serialize($copy));
    report('direct-closure', fn() => serialize($other));
    report('nested-closure', fn() => serialize(['entry' => $other]));
    break;
case 'cursor':
    $s = new SplObjectStorage; $a = item('a'); $b = item('b'); $c = item('c');
    $s[$a] = 1; $s[$b] = 2; $s[$c] = 3;
    report('initial', fn() => state($s)); $s->next();
    report('second', fn() => state($s)); $s->setInfo(22);
    report('updated', fn() => $s[$b]);
    foreach ([2, 0, 1, -1, 3] as $position) {
        report('seek', fn() => $s->seek($position)); report('position', fn() => state($s));
    }
    $s->next(); $s->next(); $s->next(); report('end', fn() => state($s));
    $s[item('tail')] = 4; report('append-at-end', fn() => state($s));
    break;
case 'comparison-reentry':
    class CompareInfo {
        public $action;
        public function __toString(): string {
            echo "convert\n";
            ($this->action)();
            return 'same';
        }
    }
    $s = new SplObjectStorage; $other = new SplObjectStorage; $a = item('a'); $b = item('b');
    $info = new CompareInfo;
    $info->action = function () use ($s, $other, $a, $b) {
        unset($s[$a]);
        $other[$b] = 8;
    };
    $s[$a] = $info; $other[$a] = 'same'; $s[$b] = 7; $other[$b] = 7;
    report('live-comparison', fn() => $s == $other);
    report('mutated', fn() => [$s->count(), $other[$b]]);
    $s = new SplObjectStorage; $other = new SplObjectStorage;
    $s[$a] = $s; $other[$a] = $other;
    report('recursive', fn() => $s == $other);
    unset($s[$a], $other[$a]);
    break;
case 'metadata':
    foreach (['offsetSet', 'offsetGet', 'offsetExists', 'offsetUnset', 'attach', 'detach',
        'contains', 'getHash', 'count', 'seek', '__serialize', '__unserialize'] as $method) {
        $reflection = new ReflectionMethod(SplObjectStorage::class, $method);
        $parameters = [];
        foreach ($reflection->getParameters() as $parameter) {
            $parameters[] = [$parameter->getName(), (string)$parameter->getType(),
                $parameter->isOptional(), $parameter->isDefaultValueAvailable() ? $parameter->getDefaultValue() : '-'];
        }
        report($method, fn() => [$reflection->getNumberOfRequiredParameters(), $parameters,
            (string)$reflection->getReturnType(), (string)$reflection->getTentativeReturnType()]);
    }
    break;
case 'cursor-removal':
    foreach (['syntax', 'method', 'legacy'] as $mode) {
        echo $mode, "\n"; $s = new SplObjectStorage;
        $a = item('a'); $b = item('b'); $c = item('c');
        $s[$a] = 1; $s[$b] = 2; $s[$c] = 3; $s->next();
        if ($mode === 'syntax') unset($s[$b]);
        elseif ($mode === 'method') $s->offsetUnset($b);
        else $s->detach($b);
        report('removed-current', fn() => state($s));
        $s->next(); report('next', fn() => state($s));
    }
    break;
case 'hash-collisions':
    class KeyStorage extends SplObjectStorage {
        public function getHash($object): string { echo 'hash:', $object->name, "\n"; return 'same'; }
    }
    $s = new KeyStorage; $a = item('a'); $b = item('b');
    $s[$a] = 7; $s[$b] = 9;
    report('collision', fn() => [$s->count(), $s->current() === $a, $s[$a], $s[$b]]);
    report('exists', fn() => $s->offsetExists($b));
    unset($s[$b]); report('empty', fn() => $s->count());
    break;
case 'unset-overrides':
    class HashUnset extends SplObjectStorage {
        public function getHash($object): string { return $object->name; }
    }
    class MethodUnset extends SplObjectStorage {
        public function offsetUnset($object): void { echo "override\n"; parent::offsetUnset($object); }
    }
    class InheritedUnset extends SplObjectStorage {}
    foreach (['HashUnset', 'MethodUnset', 'InheritedUnset'] as $class) {
        echo $class, "\n"; $s = new $class; $a = item('a'); $b = item('b'); $c = item('c');
        $s[$a] = 1; $s[$b] = 2; $s[$c] = 3; $s->next(); unset($s[$b]);
        report('cursor', fn() => state($s));
    }
    break;
case 'hash-failure':
    class HashFailure extends SplObjectStorage {
        public $fail = false;
        public function getHash($object): string {
            if ($this->fail) throw new RuntimeException('hash-stopped');
            return $object->name;
        }
    }
    $s = new HashFailure; $a = item('a'); $s[$a] = 1; $s->fail = true;
    report('write', fn() => $s->offsetSet($a, 2));
    report('read', fn() => $s->offsetGet($a));
    report('exists', fn() => $s->offsetExists($a));
    report('delete', fn() => $s->offsetUnset($a));
    $s->fail = false; report('preserved', fn() => [$s->count(), $s[$a]]);
    break;
case 'invalid-arguments':
    $s = new SplObjectStorage;
    foreach ([null, 3, 'bad', []] as $bad) {
        report('set', fn() => $s->offsetSet($bad, 2));
        report('get', fn() => $s->offsetGet($bad));
        report('exists', fn() => $s->offsetExists($bad));
        report('unset', fn() => $s->offsetUnset($bad));
        report('hash', fn() => $s->getHash($bad));
    }
    report('bulk', fn() => $s->addAll(new stdClass));
    break;
case 'bulk':
    $a = item('a'); $b = item('b'); $c = item('c');
    $s = new SplObjectStorage; $other = new SplObjectStorage;
    $s[$a] = 1; $s[$b] = 2; $other[$b] = 20; $other[$c] = 30; $s->next();
    report('add', fn() => $s->addAll($other)); report('cursor', fn() => state($s));
    report('view', fn() => view($s)); report('self-add', fn() => $s->addAll($s));
    report('except', fn() => $s->removeAllExcept($other)); report('view', fn() => view($s));
    report('remove', fn() => $s->removeAll($other)); report('empty', fn() => state($s));
    report('self-empty', fn() => $other->removeAll($other));
    break;
case 'bulk-reentry':
    class MutatingStorage extends SplObjectStorage {
        public $action;
        public function getHash($object): string {
            if ($this->action) { $action = $this->action; $this->action = null; $action($object); }
            return $object->name;
        }
    }
    $a = item('a'); $b = item('b'); $source = new SplObjectStorage;
    $source[$a] = ['first']; $source[$b] = ['second']; $s = new MutatingStorage;
    $s->action = function ($object) use ($source, $b) { unset($source[$object], $source[$b]); };
    report('copied', fn() => $s->addAll($source)); report('view', fn() => view($s));
    $other = new MutatingStorage; $other[$a] = 1;
    $other->action = function ($object) use ($s) { unset($s[$object]); };
    report('except', fn() => $s->removeAllExcept($other)); report('size', fn() => $s->count());
    break;
case 'live-iteration':
    $s = new SplObjectStorage; $a = item('a'); $b = item('b'); $c = item('c');
    $s[$a] = 1; $s[$b] = 2;
    foreach ($s as $key => $object) {
        echo $key, ':', $object->name, ':', $s->getInfo(), "\n";
        if ($object === $a) { unset($s[$b]); $s[$c] = 3; }
    }
    report('end', fn() => state($s));
    break;
case 'references-clone':
    $s = new SplObjectStorage; $a = item('a'); $info = ['value' => 1]; $s[$a] = $info;
    $info['value'] = 2; $copy = clone $s;
    $read = $s[$a]; $read['value'] = 3;
    report('cow', fn() => [$s[$a], $copy[$a], $info, $read]);
    $s->setInfo(['value' => 4]); report('independent', fn() => [$s[$a], $copy[$a]]);
    $reference =& $s[$a]; $reference = 'detached';
    report('indirect', fn() => [$s[$a], $reference]);
    break;
case 'debug-projection':
    $s = new SplObjectStorage; $a = item('a'); $s[$a] = ['value' => 1];
    $debug = $s->__debugInfo(); $key = array_key_first($debug); $rows = $debug[$key];
    report('key', fn() => bin2hex($key));
    report('data', fn() => [$rows[0]['obj'] === $a, $rows[0]['inf']]);
    $rows[0]['obj'] = item('copy'); $rows[0]['inf']['value'] = 2;
    report('detached', fn() => [$s->current() === $a, $s[$a], get_object_vars($s)]);
    break;
case 'modern-restore':
    $s = new SplObjectStorage; $a = item('a'); $b = item('b'); $s[$a] = 1;
    report('restore', fn() => $s->__unserialize([[$a, 2, $b, 3], []]));
    report('entries', fn() => view($s));
    $state = $s->__serialize(); report('shape', fn() => [count($state), $state[0][0] === $a, $state[0][1], $state[1]]);
    $copy = unserialize(serialize($s)); report('wire', fn() => view($copy));
    break;
case 'partial-restore':
    $a = item('a'); $b = item('b');
    foreach ([[], [[1], []], [[$b, 2, 7, 3], []], [[$b, 2], [], 'extra']] as $data) {
        $s = new SplObjectStorage; $s[$a] = 1;
        report('load', fn() => $s->__unserialize($data)); report('after', fn() => view($s));
    }
    break;
case 'members':
    class MemberStorage extends SplObjectStorage { public int $slot = 8; protected $hidden = 9; }
    $s = new MemberStorage; $a = item('a');
    report('load', fn() => $s->__unserialize([[$a, 1], ['slot' => 'raw']]));
    report('members', fn() => $s->__serialize()[1]);
    report('roundtrip', fn() => unserialize(serialize($s))->__serialize()[1]);
    break;
case 'retirement':
    class RetiredInfo {
        public function __destruct() {
            global $s, $a;
            echo 'retired:', $s->count(), ':', $s->offsetExists($a) ? 'present' : 'absent', "\n";
            if ($s->offsetExists($a)) echo 'new:', serialize($s[$a]), "\n";
        }
    }
    $s = new SplObjectStorage; $a = item('a'); $s[$a] = new RetiredInfo;
    $s[$a] = 'replacement'; $s[$a] = new RetiredInfo; unset($s[$a]);
    report('empty', fn() => $s->count());
    break;
case 'retirement-reentry':
    class ClearingInfo {
        public function __destruct() { global $s; echo 'clear:', $s->removeAll($s), "\n"; }
    }
    $s = new SplObjectStorage; $a = item('a'); $s[$a] = new ClearingInfo;
    $s->setInfo('published'); report('empty', fn() => $s->count());
    break;
case 'cycles':
    gc_collect_cycles();
    $s = new SplObjectStorage; $a = item('a'); $a->owner = $s; $s[$a] = $s;
    $weak = WeakReference::create($s); unset($s, $a);
    report('collected', fn() => gc_collect_cycles()); report('gone', fn() => $weak->get() === null);
    break;
case 'deprecated-aliases':
    $s = new SplObjectStorage; $a = item('a');
    report('attach', fn() => $s->attach($a, 1)); report('contains', fn() => $s->contains($a));
    report('detach', fn() => $s->detach($a));
    report('attach-invalid', fn() => $s->attach(42));
    report('detach-invalid', fn() => $s->detach(42));
    report('contains-invalid', fn() => $s->contains(42));
    break;
case 'scalar-property-boundary':
    class ScalarPropertyBox { public int $number = 1; public $value; public int $empty; }
    class RetiredPropertyValue { public function __destruct() { echo "retired\n"; } }
    function writeNumber($object, $value) { return $object->number = $value; }
    function readValue($object) { return $object->value; }
    function readNamed($object, $name) { return $object->$name; }
    function callValue($object) { return serialize($object->value); }
    $box = new ScalarPropertyBox;
    foreach ([4, 7, PHP_INT_MAX, PHP_INT_MIN] as $value)
        report('write', fn() => [writeNumber($box, $value), $box->number]);
    $alias =& $box->number;
    report('alias-write', fn() => writeNumber($box, 11)); report('alias', fn() => $alias);
    report('weak-write', fn() => writeNumber($box, '12')); report('alias-weak', fn() => $alias);
    report('bad-write', fn() => writeNumber($box, [])); report('preserved', fn() => [$alias, $box->number]);
    unset($box->number); report('reinit', fn() => writeNumber($box, 19)); report('detached', fn() => $alias);
    $box->value = new RetiredPropertyValue; $previous = readValue($box);
    $box->value = 23; unset($previous); report('long-after-object', fn() => [readValue($box), readNamed($box, 'value'), callValue($box)]);
    foreach ([false, 2.5, 'text', ['k' => 3], 31] as $value) {
        $box->value = $value; report('mixed', fn() => [readValue($box), readNamed($box, 'value'), callValue($box)]);
    }
    $reference = 43; $box->value =& $reference;
    report('reference', fn() => [readValue($box), callValue($box)]); $reference = 47;
    report('changed-reference', fn() => readValue($box));
    report('uninitialized', fn() => readNamed($box, 'empty'));
    $source = 'function readLargeProperty($object) {';
    for ($i = 0; $i < 80; $i++) $source .= '$local'.$i.' = [1];';
    $source .= 'return [$object->number, $object->value, $local79];}';
    eval($source); report('large-frame', fn() => readLargeProperty($box));
    break;
case 'method-lookup-boundary':
    class EmptyLookup {}
    class CloneLookup { public int $value = 2; public function __CLONE() { $this->value += 3; } }
    class InheritedLookup extends CloneLookup {}
    trait LookupTrait { public function MiXeD() { return 17; } }
    class TraitLookup { use LookupTrait { MiXeD as protected hidden; } public function expose() { return $this->HiDdEn(); } }
    class InheritedTraitLookup extends TraitLookup {}
    class NativeLookup extends SplObjectStorage {}
    foreach ([new stdClass, new EmptyLookup, new InheritedLookup, new InheritedTraitLookup, new NativeLookup] as $object) {
        $copy = clone $object;
        report('clone', fn() => [get_class($copy), $copy !== $object]);
        report('missing', fn() => method_exists($copy, 'absentMethod'));
    }
    $original = new InheritedLookup; $copy = clone $original;
    report('inherited-hook', fn() => [$original->value, $copy->value]);
    $trait = new InheritedTraitLookup;
    report('trait', fn() => [$trait->mixed(), $trait->expose()]);
    report('protected', fn() => $trait->hidden());
    $storage = new NativeLookup; $key = item('key'); $storage[$key] = 'info';
    $copy = clone $storage;
    report('native', fn() => [$copy->CoUnT(), $copy->offsetGet($key), $storage->offsetGet($key)]);
    break;
case 'wire-integer-boundary':
    foreach ([PHP_INT_MIN, -10000, -1, 0, 9, 10, 99, 100, PHP_INT_MAX] as $number)
        echo serialize([$number => $number]), "\n";
    $shared = ['big' => PHP_INT_MIN]; $object = (object)['number' => PHP_INT_MAX];
    $value = array_fill(0, 1000, null);
    $value[] =& $shared; $value[] =& $shared; $value[] = $object; $value[] = $object;
    $value[] = "\0\xc3\xa9\xff";
    $wire = serialize($value);
    report('large', fn() => [strlen($wire), md5($wire), serialize(unserialize($wire)) === $wire]);
    break;
case 'numeric-assignment-boundary':
    foreach (['0', '-0', '+17', '-023', '9223372036854775807', '-9223372036854775808',
        '9223372036854775808', ' 27 ', "\t-31\r\n", '1.5', '2e1', '4tail', 'not numeric', ''] as $text) {
        report('numeric', function () use ($text) { $value = 0; $value += $text; return $value; });
    }
    break;
case 'owned-assignment-boundary':
    function ownedSource() { return ['payload' => 1]; }
    function &ownedReference(&$value) { return $value; }
    $target = null; $target = ownedSource(); $copy = $target; $copy['payload'] = 2;
    report('array-cow', fn() => [$target, $copy]);
    $target = false; $target = "\0\xff"; report('binary', fn() => bin2hex($target));
    $target = 0; $target = item('shared'); $copy = $target; $target = null;
    report('object', fn() => $copy->name);
    $target = null; $target = static fn() => 23; $copy = $target; $target = false;
    report('closure', fn() => $copy());
    $value = ['payload' => 3]; $target = null; $target = ownedReference($value); $target['payload'] = 4;
    report('reference-read', fn() => [$value, $target]);
    $handles = [];
    for ($i = 0; $i < 12; $i++) {
        $target = null; $target = fopen('php://memory', 'w+'); $copy = false; $copy = $target;
        $handles[] = [$target, $copy];
    }
    $closed = 0; $retired = 0;
    foreach ([7, 0, 11, 3, 9, 1, 5, 10, 2, 8, 4, 6] as $index) {
        $closed += (int)fclose($handles[$index][0]);
        $retired += (int)!is_resource($handles[$index][1]);
    }
    report('resource-alias', fn() => [$closed, $retired]);
    $source = 'function moveWideOwners($value) {';
    for ($i = 0; $i < 80; $i++) $source .= '$local'.$i.' = null;';
    $source .= '$array = null; $array = ownedSource();';
    $source .= '$stream = null; $stream = fopen("php://memory", "w+");';
    $source .= '$held = null; $held = [$stream];';
    $source .= '$copy = null; $copy = ownedReference($value); $copy["payload"] = 9;';
    $source .= 'return [$array, $copy, $value, is_resource($held[0]), fclose($stream), is_resource($held[0])];}';
    eval($source); report('wide-owning-temporaries', fn() => moveWideOwners(['payload' => 5]));
    break;
case 'scalar-call-operands-boundary':
    function scalarOperand($value) { return ($value * 3) + 2; }
    function scalarPair($left, $right) { return ($left * 3) + $right; }
    function scalarSource($value) { echo 'source;'; return $value; }
    foreach ([0, 7, -11, PHP_INT_MAX, 1.5, '17'] as $value) {
        report('cv', fn() => scalarOperand($value));
        report('temporary', fn() => scalarOperand(scalarSource($value)));
    }
    report('literal', fn() => scalarOperand(23));
    $number = 31; $alias =& $number;
    report('reference-cv', fn() => scalarOperand($alias));
    $number = 37; report('changed-reference', fn() => scalarOperand($alias));
    report('named', fn() => scalarPair(right: 41, left: 43));
    report('invalid', fn() => scalarOperand([]));
    report('after-error', fn() => scalarOperand(47));
    break;
case 'constructor-publication-boundary':
    class ConstructedScalars {
        public int $number; public $payload = ['default'];
        public function __construct(int $number, $payload) { $this->number = $number; $this->payload = $payload; }
    }
    function constructScalars($number, $payload) { return new ConstructedScalars($number, $payload); }
    foreach ([null, false, true, PHP_INT_MIN, PHP_INT_MAX, -0.0, 1.5, 'text', ['entry' => 3]] as $payload) {
        for ($i = 0; $i < 3; $i++) $box = constructScalars($i, $payload);
        report('constructed', fn() => [$box->number, $box->payload]);
    }
    $source = ['entry' => 5]; $box = constructScalars(7, $source); $box->payload['entry'] = 6;
    report('cow', fn() => [$source, $box->payload]);
    $value = 11; $alias =& $value; $box = constructScalars($alias, $alias); $value = 13;
    report('reference-copy', fn() => [$box->number, $box->payload, $value]);
    report('weak-number', fn() => constructScalars('17', 0)->number);
    report('bad-number', function () {
        try { constructScalars([], null); }
        catch (TypeError $error) {
            return [get_class($error), strstr($error->getMessage(), ', called in', true),
                str_contains($error->getMessage(), __FILE__)];
        }
    });
    $code = 'function constructWideScalars($input) {';
    for ($i = 0; $i < 80; $i++) $code .= '$local'.$i.' = [1];';
    $code .= '$result = []; for ($i = 0; $i < 3; $i++) { $box = new ConstructedScalars($i, $input); $result[] = [$box->number, $box->payload]; } return [$result, $local79];}';
    eval($code); report('wide-constructor', fn() => constructWideScalars('kept'));
    class ConstructedRetirement { public function __destruct() { echo "constructor-retired\n"; } }
    $box = constructScalars(19, new ConstructedRetirement); $copy = $box; unset($box);
    report('retained', fn() => $copy->number); unset($copy);
    class UnpackedBoundary {
        public $payload; public $label;
        public function __construct(int &$value, array $payload, string $label = 'default') {
            ++$value; $this->payload = $payload; $this->label = $label;
            echo 'unpacked:', $label, "\n";
            if ($label === 'failure') throw new RuntimeException('unpacked failure');
        }
        public function __destruct() { echo 'unpacked-retired:', $this->label, "\n"; }
    }
    class InheritedUnpackedBoundary extends UnpackedBoundary {}
    $number = 23; $payload = ['entry' => 5]; $args = [&$number, $payload, 'positional'];
    $box = new InheritedUnpackedBoundary(...$args); $box->payload['entry'] = 7;
    report('unpacked-cow-reference', fn() => [$number, $payload, $args[1], $box->payload]);
    unset($box);
    $args = ['label' => 'named', 'payload' => $payload, 'value' => &$number];
    $box = new UnpackedBoundary(...$args);
    report('unpacked-named', fn() => [$number, $box->label, $box->payload]); unset($box);
    report('unpacked-failure', function () use (&$number) {
        try { new UnpackedBoundary(...[&$number, [], 'failure']); }
        catch (RuntimeException $error) {
            return [$error->getMessage(), $error->getFile() === __FILE__, $error->getLine() > 0, $number];
        }
    });
    class WithoutUnpackedConstructor {}
    report('unpacked-no-constructor', fn() => get_class(new WithoutUnpackedConstructor(...[1, 2])));
    $iterator = new ArrayIterator(...[['entry' => 31]]);
    report('unpacked-native', fn() => [$iterator->current(), $iterator->key()]);
    break;
case 'primitive-assignment-boundary':
    function assignPrimitiveValues($values) {
        $target = null; $result = [];
        foreach ($values as $source) {
            $target = $source; $copy = $target; $copy = $copy;
            $result[] = $copy;
        }
        return $result;
    }
    $values = [null, false, true, PHP_INT_MIN, PHP_INT_MAX, -0.0, 1.25, INF, -INF, NAN];
    report('primitive', fn() => assignPrimitiveValues($values));
    report('mixed', fn() => assignPrimitiveValues(['text', ['x' => 2], 3, null]));
    function &primitiveReference(&$value) { return $value; }
    $number = 7; $copy = primitiveReference($number); $copy = 11;
    report('read-reference', fn() => [$number, $copy]);
    $alias =& $number; $alias = 13; report('write-reference', fn() => [$number, $alias]);
    class AssignedTypedNumber { public int $number = 1; }
    $typed = new AssignedTypedNumber; $alias =& $typed->number;
    $alias = '17'; report('coerced-reference', fn() => [$typed->number, $alias]);
    report('invalid-reference', function () use (&$alias) { $alias = []; });
    report('preserved-reference', fn() => [$typed->number, $alias]);
    class AssignmentRetired { public function __destruct() { echo "assignment-retired\n"; } }
    $target = new AssignmentRetired; $weak = WeakReference::create($target);
    $target = 19; report('retired', fn() => [$target, $weak->get()]);
    $source = 'function assignWidePrimitive($input) {';
    for ($i = 0; $i < 80; $i++) $source .= '$local'.$i.' = [1];';
    $source .= '$target = 0; $target = $input; $local79 = 23; return [$target, $local79];}';
    eval($source); report('wide', fn() => assignWidePrimitive(-0.0));
    break;
default: throw new RuntimeException('unknown specimen');
}
