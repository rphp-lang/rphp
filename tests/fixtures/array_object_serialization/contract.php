<?php
// Independent native-state serialization specimens, one request per case.
set_error_handler(function ($code, $message) {
    echo 'diagnostic:', $code, ':', $message, "\n";
    return true;
});
function attempt($operation) {
    try { $operation(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
function state($object) {
    echo get_class($object), ':', $object->getFlags(), ':', json_encode($object->getArrayCopy()), "\n";
    if ($object instanceof ArrayObject) echo 'iterator:', $object->getIteratorClass(), "\n";
}
switch (getenv('RPHP_ARRAY_SERIALIZATION_CASE')) {
case 'readonly-members':
    require __DIR__ . '/readonly-members.php';
    break;
case 'stream-byte-provenance':
    require __DIR__ . '/stream-byte-provenance.php';
    break;
case 'modern-state':
    class StateCursor extends ArrayIterator {}
    class StateContainer extends ArrayObject {
        private string $hidden = 'private-state';
        protected int $number = 17;
        public string $label = 'public-state';
        public function hidden() { return $this->hidden; }
    }
    foreach ([new ArrayObject(['alpha' => 13], 2, StateCursor::class), new ArrayIterator(['beta' => 29], 1), new StateContainer(['gamma' => 31], 3, StateCursor::class)] as $object) {
        attempt(function () use ($object) {
            $data = $object->__serialize();
            echo json_encode($data), "\n";
            $copy = unserialize(serialize($object));
            state($copy);
            if ($copy instanceof StateContainer) echo $copy->hidden(), ':', $copy->label, "\n";
        });
    }
    break;
case 'modern-validation':
    $object = new ArrayObject(['retained' => 41], 2);
    foreach ([[], [false, [], []], [0, 9, []], [0, [], 9], [0, [], [], 9], [0, [], [], 'AbsentStateCursor'], [0, [], [], 'stdClass'], [1, ['fresh' => 43], []], [2, ['last' => 47], [], null]] as $input) {
        attempt(function () use ($object, $input) { $object->__unserialize($input); echo "restored\n"; });
        state($object);
    }
    attempt(function () use ($object) { $object->__unserialize('wrong'); });
    state($object);
    break;
case 'legacy-state':
    foreach (['ArrayObject', 'ArrayIterator'] as $class) {
        $object = new $class(['one' => 53, 'two' => ['v' => 59]], 2);
        attempt(function () use ($object, $class) {
            $wire = $object->serialize();
            echo bin2hex($wire), "\n";
            $copy = new $class(['old' => 61], 1);
            $copy->unserialize($wire);
            state($copy);
            $copy['two']['v'] = 67;
            echo json_encode($object->getArrayCopy()), ':', json_encode($copy->getArrayCopy()), "\n";
        });
    }
    break;
case 'legacy-validation':
    $object = new ArrayObject(['kept' => 71], 2);
    foreach (['', 'broken', 'x:i:0;', 'x:i:0;a:0:{};m:a:0:{}', 'x:i:0;a:0:{};m:a:0:{}trailing', 'x:i:0;i:7;;m:a:0:{}', 'x:i:0;r:99;;m:a:0:{}'] as $wire) {
        echo 'input:', bin2hex($wire), "\n";
        attempt(function () use ($object, $wire) { $object->unserialize($wire); echo "restored\n"; });
        state($object);
    }
    break;
case 'reference-identity':
    $cell = ['n' => 73];
    $leaf = (object)['n' => 79];
    $object = new ArrayObject(['left' => &$cell, 'right' => &$cell, 'first' => $leaf, 'second' => $leaf]);
    $object['self'] = $object;
    attempt(function () use ($object) {
        $copy = unserialize(serialize($object));
        var_dump($copy['first'] === $copy['second'], $copy['self'] === $copy);
        $copy['left']['n'] = 83;
        echo $copy['right']['n'], ':', $object['right']['n'], "\n";
        $manual = new ArrayObject;
        $manual->unserialize($object->serialize());
        var_dump($manual['first'] === $manual['second'], $manual['self'] === $manual, $manual['self']['self'] === $manual['self']);
        $manual['right']['n'] = 89;
        echo $manual['left']['n'], ':', $object['left']['n'], "\n";
    });
    break;
case 'binary-state':
    $key = hex2bin('00ff80');
    $value = hex2bin('ff007f80');
    foreach (['ArrayObject', 'ArrayIterator'] as $class) {
        $object = new $class([$key => $value]);
        attempt(function () use ($object, $class, $key) {
            echo bin2hex($object->serialize()), "\n";
            $copy = unserialize(serialize($object));
            echo bin2hex(array_key_first($copy->getArrayCopy())), ':', bin2hex($copy[$key]), "\n";
            $manual = new $class;
            $manual->unserialize($object->serialize());
            echo bin2hex($manual[$key]), "\n";
        });
    }
    break;
case 'sorting-guard':
    foreach (['ArrayObject', 'ArrayIterator'] as $class) {
        $object = new $class(['z' => 97, 'a' => 101]);
        $object->uksort(function ($left, $right) use ($object) {
            attempt(function () use ($object) { $object->__unserialize([0, [], []]); });
            attempt(function () use ($object) { $object->unserialize('x:i:0;a:0:{};m:a:0:{}'); });
            return $left <=> $right;
        });
        state($object);
    }
    break;
case 'callback-state':
    class HookContainer extends ArrayObject {
        public string $note = 'before';
        public function __serialize(): array {
            echo "serialize-hook\n";
            $data = parent::__serialize();
            $this->note = 'after';
            return $data;
        }
        public function __unserialize(array $data): void {
            echo "unserialize-hook\n";
            parent::__unserialize($data);
            echo $this->note, "\n";
        }
    }
    $object = new HookContainer(['entry' => 103], 1);
    attempt(function () use ($object) {
        $wire = serialize($object);
        echo $object->note, "\n";
        $copy = unserialize($wire);
        state($copy);
    });
    break;
case 'legacy-reference-table':
    foreach ([4, 5] as $reference) {
        $payload = 'x:i:2;a:0:{};m:a:0:{}';
        $wire = 'a:2:{i:0;C:11:"ArrayObject":' . strlen($payload) . ':{' . $payload . '}i:1;R:' . $reference . ';}';
        attempt(function () use ($wire) {
            $copy = unserialize($wire);
            $copy[1]['later'] = 107;
            echo json_encode($copy[0]->__serialize()), ':', json_encode($copy[1]), "\n";
        });
    }
    break;
case 'legacy-diagnostic-origin':
    set_error_handler(function ($level, $message, $file, $line) {
        echo $level, ':', $message, "\n";
        var_dump($file === __FILE__, $line > 0);
        return true;
    });
    $payload = 'x:i:0;O:8:"stdClass":0:{};m:a:0:{}';
    $copy = unserialize('C:11:"ArrayObject":'.strlen($payload).':{'.$payload.'}');
    echo json_encode($copy->__serialize()), "\n";
    break;
case 'member-isolation':
    $object = new ArrayObject;
    $object->__unserialize([0, [149], ['storage' => 'plain', "\0ArrayObject\0storage" => 'hidden']]);
    echo json_encode($object->__serialize()), ':', json_encode($object->getArrayCopy()), "\n";
    $copy = unserialize(serialize($object));
    echo json_encode($copy->__serialize()), ':', json_encode($copy->getArrayCopy()), "\n";
    break;
case 'member-release':
    class ReleasedMember {
        public function __destruct() { global $owner; echo 'released:', gettype($owner->member), "\n"; }
    }
    class ReleaseOwner extends ArrayObject { public $member; }
    $owner = new ReleaseOwner;
    $owner->member = new ReleasedMember;
    $owner->__unserialize([0, ['retained' => 151], ['member' => 'modern']]);
    $owner->member = new ReleasedMember;
    $owner->unserialize('x:i:0;a:0:{};m:a:1:{s:6:"member";s:6:"legacy";}');
    echo $owner->member, "\n";
    break;
case 'legacy-callback-order':
    class MemberSnapshot extends ArrayObject { public string $note = 'initial'; }
    class StorageCallback {
        public function __serialize(): array {
            global $owner;
            $owner->note = 'changed-by-storage';
            return ['entry' => 109];
        }
    }
    $owner = new MemberSnapshot([new StorageCallback]);
    echo $owner->serialize(), "\n";
    break;
case 'self-state':
    class SelfState extends ArrayObject {
        public function __construct() { parent::__construct($this); }
    }
    $self = new SelfState;
    $self['entry'] = 'self-value';
    echo json_encode($self->__serialize()), "\n";
    $copy = unserialize(serialize($self));
    echo json_encode($copy->__serialize()), ':', $copy['entry'], "\n";
    $manual = new ArrayObject;
    $manual->unserialize($self->serialize());
    echo json_encode($manual->__serialize()), ':', $manual['entry'], "\n";
    break;
case 'wire-identity':
    $leaf = (object)['entry' => 113];
    $object = new ArrayObject([$leaf, $leaf]);
    $object['self'] = $object;
    var_dump($object[0] === $object[1]);
    echo bin2hex($object->serialize()), "\n";
    $reference = $leaf;
    $explicit = new ArrayObject([&$reference, &$reference]);
    echo bin2hex(serialize($explicit)), "\n";
    break;
case 'validation-mutation':
    $object = new ArrayObject(['kept' => 127], 2);
    foreach (['x:i:0;a:1:{s:5:"fresh";i:131;}', 'x:i:1;a:0:{};m:i:137;', 'x:i:2;a:0:{};m:a:1:{i:7;i:139;}'] as $wire) {
        attempt(function () use ($object, $wire) { $object->unserialize($wire); });
        echo json_encode($object->__serialize()), "\n";
    }
    $payload = 'x:i:0;a:0:{};m:a:0:{}';
    var_dump(unserialize('C:11:"ArrayObject":'.strlen($payload).':0'.$payload.'}'));
    break;
default:
    throw new Exception('Unknown original specimen');
}
