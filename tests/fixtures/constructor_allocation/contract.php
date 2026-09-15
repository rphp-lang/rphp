<?php
error_reporting(E_ALL);
function emit($value) { echo json_encode($value), "\n"; }
function diagnostic($level, $message) { emit(['diagnostic', $level, $message]); return true; }
set_error_handler('diagnostic');
class Parcel {
    public $first;
    public $second;
    public function __construct($first = null, $second = null) {
        $this->first = $first; $this->second = $second;
        emit(['parcel', spl_object_id($this)]);
    }
}
function token($label) {
    $object = new stdClass;
    emit([$label, spl_object_id($object)]);
    return $object;
}
function stopArgument() { echo "argument-stop\n"; throw new LogicException('argument'); }
function observe($body) {
    try { $body(); } catch (Throwable $error) { emit([get_class($error), $error->getMessage()]); }
}
switch (getenv('RPHP_CONSTRUCTOR_ALLOCATION_CASE')) {
case 'nested':
    $outer = new Parcel(new Parcel(new stdClass), new stdClass);
    emit([spl_object_id($outer), spl_object_id($outer->first), spl_object_id($outer->first->first), spl_object_id($outer->second)]);
    break;
case 'argument_callbacks':
    $outer = new Parcel(token('left'), token('right'));
    emit([spl_object_id($outer), spl_object_id($outer->first), spl_object_id($outer->second)]);
    break;
case 'dynamic_owner':
    function owner() { echo "owner\n"; return Parcel::class; }
    $outer = new (owner())(token('child'));
    emit([spl_object_id($outer), spl_object_id($outer->first)]);
    break;
case 'anonymous_owner':
    $outer = new class(token('child')) extends Parcel {};
    emit([spl_object_id($outer), spl_object_id($outer->first)]);
    break;
case 'no_constructor':
    class Plain { public function __destruct() { echo "plain-drop\n"; } }
    $outer = new Plain(token('discarded'));
    emit(spl_object_id($outer)); unset($outer); echo "after\n";
    break;
case 'unpacked':
    function arguments() { echo "unpack\n"; return [token('first'), token('second')]; }
    $outer = new Parcel(...arguments());
    emit([spl_object_id($outer), spl_object_id($outer->first), spl_object_id($outer->second)]);
    break;
case 'named_references':
    class ReferenceParcel {
        public $item; public $number;
        public function __construct(&$number, $item) { $this->number =& $number; $this->item = $item; $number += 2; }
    }
    $number = 4;
    $outer = new ReferenceParcel(item: token('child'), number: $number);
    $number += 1; emit([spl_object_id($outer), spl_object_id($outer->item), $number, $outer->number]);
    break;
case 'unpacked_references':
    class ReferenceParcel {
        public $item;
        public function __construct(&$number, $item) { $number += 3; $this->item = $item; }
    }
    function referenceArguments(&$number) { return [&$number, token('child')]; }
    $number = 5;
    $outer = new ReferenceParcel(...referenceArguments($number));
    emit([spl_object_id($outer), spl_object_id($outer->item), $number]);
    break;
case 'defaults':
    function envelope($value = new Parcel(new Parcel(new stdClass))) {
        emit([spl_object_id($value), spl_object_id($value->first), spl_object_id($value->first->first)]);
    }
    envelope(); envelope();
    break;
case 'validation_priority':
    class Locked { private function __construct($value) {} }
    abstract class AbstractParcel {}
    observe(function () { new Locked(token('must-not-run')); });
    observe(function () { new AbstractParcel(token('must-not-run')); });
    observe(function () { new MissingParcel(token('must-not-run')); });
    echo "after\n";
    break;
case 'autoload_order':
    function loadParcel($class) {
        echo "load:$class\n";
        $GLOBALS['retained'] = token('loader');
        eval('class LoadedParcel extends Parcel {}');
    }
    spl_autoload_register('loadParcel');
    $outer = new LoadedParcel(token('child'));
    emit([spl_object_id($outer), spl_object_id($outer->first), spl_object_id($GLOBALS['retained'])]);
    break;
case 'argument_throw':
    class NeverFinished {
        public function __construct($value) { echo "must-not-construct\n"; }
        public function __destruct() { echo "must-not-destroy\n"; }
    }
    try { new NeverFinished(stopArgument()); }
    catch (Throwable $error) { emit([get_class($error), $error->getMessage()]); }
    $after = token('after'); emit(spl_object_id($after));
    break;
case 'constructor_throw':
    class Rejected {
        public function __construct($value) {
            emit(['reject', spl_object_id($this), spl_object_id($value)]);
            $GLOBALS['weak'] = WeakReference::create($this);
            throw new LogicException('constructor');
        }
        public function __destruct() { echo "must-not-destroy\n"; }
    }
    try { new Rejected(token('child')); }
    catch (Throwable $error) { emit([get_class($error), $error->getMessage()]); }
    emit($GLOBALS['weak']->get()); $after = token('after');
    break;
case 'no_constructor_throw':
    class Plain { public function __destruct() { echo "plain-drop\n"; } }
    observe(function () { new Plain(stopArgument()); });
    echo "after\n";
    break;
case 'reused_handles':
    for ($i = 0; $i < 3; $i++) {
        $outer = new Parcel(new stdClass);
        emit([spl_object_id($outer), spl_object_id($outer->first)]);
        unset($outer);
    }
    break;
case 'suspended_argument':
    function building() { return new Parcel(yield 'waiting'); }
    $generator = building(); emit($generator->current());
    $child = token('resume'); $generator->send($child); $outer = $generator->getReturn();
    emit([spl_object_id($outer), spl_object_id($outer->first)]);
    break;
case 'collect_during_argument':
    function collecting() {
        $cycle = new stdClass; $cycle->self = $cycle; $weak = WeakReference::create($cycle);
        unset($cycle); gc_collect_cycles(); emit($weak->get());
        return token('after-collection');
    }
    $outer = new Parcel(collecting());
    emit([spl_object_id($outer), spl_object_id($outer->first)]);
    break;
case 'diagnostic_allocation':
    function retainedDiagnostic($level, $message) {
        echo "notice\n"; $GLOBALS['notice'] = new stdClass; return true;
    }
    set_error_handler('retainedDiagnostic');
    $outer = new Parcel($missing);
    emit([spl_object_id($outer), spl_object_id($GLOBALS['notice']), $outer->first]);
    break;
case 'argument_snapshot':
    $value = ['original']; $copy = $value;
    $outer = new Parcel($value, $value = ['changed']);
    emit([$outer->first, $outer->second, $value, $copy]);
    break;
default:
    throw new LogicException('unknown specimen');
}
