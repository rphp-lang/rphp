<?php
// Original cursor/protocol specimens; each case runs in a fresh request.
set_error_handler(function ($level, $message) {
    if ($level === E_DEPRECATED && str_contains($message, 'Using an object as a backing array')) return true;
    echo "diagnostic:", $message, "\n";
    return true;
});
function position($it) {
    echo json_encode([$it->valid(), $it->key(), $it->current()]), "\n";
}
function attempt($operation) {
    try { $operation(); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
switch (getenv('RPHP_ITERATOR_CASE')) {
case 'foreach-created-reference-sources':
    foreach (new ArrayIterator([2, 4]) as &$value) $value *= 3;
    var_dump($value);
    unset($value);
    $class = 'ArrayIterator';
    foreach (new $class([5, 7]) as &$value) ++$value;
    var_dump($value);
    unset($value);
    foreach (new class([9, 11]) extends ArrayIterator {} as &$value) $value += 4;
    var_dump($value);
    unset($value);
    class CreatedReadOnlyIterator implements Iterator {
        function rewind(): void { echo "unexpected-rewind\n"; }
        function next(): void {}
        function valid(): bool { return false; }
        function key(): mixed { return 0; }
        function current(): mixed { return 1; }
    }
    $value = 'untouched';
    attempt(function () use (&$value) {
        foreach (new CreatedReadOnlyIterator() as &$value) echo 'unexpected-body';
    });
    var_dump($value);
    break;
case 'foreach-release-projections':
    class ForeachProjectionVictim {
        function __destruct() { echo "payload-drop\n"; }
    }
    class ForeachProjectionAggregate implements IteratorAggregate {
        function __construct(public ArrayIterator $iterator, public string $mode) {}
        function getIterator(): Traversable { return $this->iterator; }
        function __destruct() {
            echo "aggregate-begin\n";
            if ($this->mode === 'append') $this->iterator[] = 19;
            else unset($this->iterator['first']);
            echo "aggregate-end\n";
            if ($this->mode === 'throw') throw new Error('aggregate-release');
        }
    }
    foreach ([false, true] as $byReference) {
        echo $byReference ? "reference\n" : "value\n";
        $iterator = new ArrayIterator(['first' => new ForeachProjectionVictim, 'last' => 7]);
        if ($byReference) {
            foreach (new ForeachProjectionAggregate($iterator, 'remove') as &$value) {
                echo 'body:', $value, "\n";
                $value += 10;
            }
            unset($value);
        } else {
            foreach (new ForeachProjectionAggregate($iterator, 'remove') as $value) {
                echo 'body:', $value, "\n";
            }
        }
        echo json_encode($iterator->getArrayCopy()), "\n";
    }
    $iterator = new ArrayIterator();
    foreach (new ForeachProjectionAggregate($iterator, 'append') as $value) echo 'unexpected-body';
    echo json_encode($iterator->getArrayCopy()), "\n";
    $iterator = new ArrayIterator(['first' => new ForeachProjectionVictim, 'last' => 8]);
    $value = 'unchanged';
    attempt(function () use ($iterator, &$value) {
        foreach (new ForeachProjectionAggregate($iterator, 'throw') as $value) echo 'unexpected-body';
    });
    var_dump($value);
    echo json_encode($iterator->getArrayCopy()), "\n";
    break;
case 'foreach-keyless-projections':
    $bytes = hex2bin('80ff');
    $iterator = new ArrayIterator([$bytes => 11, 'tail' => 22]);
    $sum = 0;
    foreach ($iterator as $value) $sum += $value;
    echo $sum, "\n";
    foreach ($iterator as &$value) ++$value;
    unset($value);
    foreach ($iterator as $key => $value) echo bin2hex($key), ':', $value, "\n";
    $source = ['heap' => ['v' => 1]];
    $iterator = new ArrayIterator($source);
    foreach ($iterator as $value) $value['v'] = 8;
    echo json_encode([$source, $iterator->getArrayCopy()]), "\n";
    foreach ($iterator as &$value) $value['v'] = 9;
    unset($value);
    echo json_encode([$source, $iterator->getArrayCopy()]), "\n";
    break;
case 'position':
    $it = new ArrayIterator(['first' => 12, 8 => 23]);
    position($it); $it->next(); position($it); $it->next(); position($it);
    $it->next(); $it->next(); $it['late'] = 34; position($it);
    $it->rewind(); position($it);
    $empty = new ArrayIterator(); $empty->next(); position($empty);
    $empty[] = 45; position($empty);
    break;
case 'seek':
    $it = new ArrayIterator(['red' => 14, 'green' => 25, 'blue' => 36]);
    foreach ([1, -1, 9, 0, '2', 1.0] as $offset) {
        attempt(function () use ($it, $offset) { var_dump($it->seek($offset)); });
        position($it);
    }
    attempt(function () use ($it) { $it->seek([]); }); position($it);
    attempt(function () use ($it) { $it->seek(); });
    attempt(function () use ($it) { $it->next(1); });
    break;
case 'mutation':
    $it = new ArrayIterator(['a'=>11, 'b'=>22, 'c'=>33, 'd'=>44]);
    $it->seek(2); unset($it['a']); position($it);
    unset($it['c']); position($it); $it->next(); position($it);
    $it['e']=55; position($it); $it['e']=66; position($it);
    $it->rewind(); unset($it['b'], $it['d']); position($it);
    $it->asort(); position($it); $it->append(77); position($it);
    echo json_encode($it->getArrayCopy()), "\n";
    break;
case 'shared':
    $owner = new ArrayObject(['x'=>17, 'y'=>28, 'z'=>39]);
    $a=$owner->getIterator(); $b=$owner->getIterator();
    $a->next(); position($a); position($b);
    unset($owner['x']); position($a); position($b);
    unset($owner['y']); position($a); position($b);
    $c=new ArrayIterator($a); $c->next(); position($a); position($c);
    $owner->exchangeArray(['p'=>41,'q'=>52]); position($a); position($b); position($c);
    $owner['r']=63; position($c);
    break;
case 'clone':
    $it=new ArrayIterator(['x'=>17,'y'=>28,'z'=>39]); $it->next();
    $copy=clone $it; position($copy); position($it);
    $copy['x']=70; echo json_encode($it->getArrayCopy()), "\n";
    $it->__construct(['new'=>91]); position($it); position($copy);
    $owner=new ArrayObject(['left'=>1,'right'=>2]); $view=$owner->getIterator();
    $view->next(); $detached=clone $view; position($detached);
    $owner['left']=3; echo json_encode($detached->getArrayCopy()), "\n";
    break;
case 'nested':
    $it=new ArrayIterator(['east'=>4,'west'=>7]);
    foreach ($it as $k=>$v) {
        foreach ($it as $j=>$w) echo "$k=$v/$j=$w\n";
    }
    position($it);
    $owner=new ArrayObject(['east'=>4,'west'=>7]);
    foreach ($owner as $k=>$v) {
        foreach ($owner as $j=>$w) echo "$k=$v/$j=$w\n";
    }
    break;
case 'references':
    $seed=['a'=>3,'b'=>5]; $it=new ArrayIterator($seed);
    foreach ($it as &$v) { $v*=10; } $v=80;
    echo json_encode([$seed,$it->getArrayCopy()]), "\n";
    $owner=new ArrayObject(['a'=>2,'b'=>4]);
    foreach ($owner as $key=>&$outer) {
        foreach ($owner as $innerKey=>&$inner) {
            if ($key==='a' && $innerKey==='a') $inner=90;
            echo "$key=$outer/$innerKey=$inner\n";
        }
    }
    unset($inner,$outer,$v);
    $external=11; $it=new ArrayIterator(['ref'=>&$external,'plain'=>22]);
    foreach ($it as &$v) $v++;
    echo $external, ':', json_encode($it->getArrayCopy()), "\n";
    break;
case 'live-foreach':
    $it=new ArrayIterator(['a'=>1,'b'=>2,'c'=>3]);
    foreach ($it as $key=>$value) {
        echo "$key=$value\n";
        if ($key==='a') { $it['b']=20; $it['d']=4; }
        if ($key==='b') unset($it['c']);
    }
    position($it);
    $it->rewind(); foreach ($it as $key=>$value) { echo "$key=$value\n"; unset($it[$key]); }
    echo json_encode($it->getArrayCopy()), "\n";
    break;
case 'object':
    #[AllowDynamicProperties]
    class CursorRecord {
        private $secret=91;
        public $left=12;
        public $middle=null;
        public $right=34;
        public int $absent;
    }
    $record=new CursorRecord(); $it=new ArrayIterator($record);
    position($it); $it->next(); position($it);
    unset($record->left); position($it); unset($record->middle); position($it);
    $record->extra=56; $it->next(); position($it);
    $it->rewind(); $it->seek(1); position($it);
    $it->setFlags(ArrayIterator::STD_PROP_LIST); position($it);
    break;
case 'object-unset':
    class CursorUnsetRecord { public $a=7; public $b=8; public $c=9; }
    $record=new CursorUnsetRecord(); $owner=new ArrayObject($record);
    $a=$owner->getIterator(); $b=$owner->getIterator();
    $a->next(); $b->next(); unset($owner['b']);
    position($a); position($b);
    $record->b=80; $a->seek(1); position($a);
    unset($record->b); position($a); $a->next(); position($a);
    $a->rewind(); position($a);
    $record=new CursorUnsetRecord(); $a=new ArrayIterator($record); $b=new ArrayIterator($record);
    $a->next(); $b->next(); unset($a['b']);
    position($a); position($b); $a['b']=81; position($a); position($b);
    break;
case 'byte-keys':
    $key=hex2bin('80ff'); $it=new ArrayIterator([$key=>'raw', 'é'=>'text']);
    for ($it->rewind(); $it->valid(); $it->next()) {
        echo bin2hex($it->key()), ':', $it->current(), "\n";
    }
    foreach ($it as $k=>&$v) { echo bin2hex($k), "\n"; $v.='!'; }
    $it->rewind(); echo bin2hex($it->key()), ':', $it->current(), "\n";
    break;
case 'overrides':
    class LoggedCursor extends ArrayIterator {
        function rewind(): void { echo "rewind\n"; parent::rewind(); }
        function valid(): bool { echo "valid\n"; return parent::valid(); }
        function current(): mixed { echo "current\n"; return parent::current(); }
        function key(): string|int|null { echo "key\n"; return parent::key(); }
        function next(): void { echo "next\n"; parent::next(); }
    }
    $it=new LoggedCursor(['k'=>6]);
    foreach ($it as $k=>$v) echo "$k=$v\n";
    echo "values-only\n"; foreach ($it as $v) echo "$v\n";
    echo "by-reference\n"; attempt(function () use ($it) { foreach ($it as &$v) $v=7; });
    echo json_encode($it->getArrayCopy()), "\n";
    break;
case 'drivers':
    class DriverCursor extends ArrayIterator {
        function rewind(): void { echo "R"; parent::rewind(); }
        function valid(): bool { echo "V"; return parent::valid(); }
        function current(): mixed { echo "C"; return parent::current(); }
        function key(): string|int|null { echo "K"; return parent::key(); }
        function next(): void { echo "N"; parent::next(); }
    }
    $it=new DriverCursor(['a'=>2,'b'=>5]);
    echo json_encode(iterator_to_array($it)), "\n";
    echo json_encode(iterator_to_array($it,false)), "\n";
    echo iterator_count($it), "\n";
    $calls=0;
    echo iterator_apply($it, function ($suffix) use (&$calls) { echo $suffix; return ++$calls<2; }, ['!']), "\n";
    echo "calls=$calls\n";
    break;
case 'exceptions':
    class FailingCursor extends ArrayIterator {
        public $fail='';
        function rewind(): void { if ($this->fail==='rewind') throw new Exception('rewind stopped'); parent::rewind(); }
        function current(): mixed { if ($this->fail==='current') throw new Exception('current stopped'); return parent::current(); }
        function next(): void { if ($this->fail==='next') throw new Exception('next stopped'); parent::next(); }
    }
    $it=new FailingCursor(['one'=>8,'two'=>9]);
    foreach (['rewind','current','next'] as $where) {
        $it->fail=$where; attempt(function () use ($it) { iterator_to_array($it); });
        $it->fail=''; position($it);
    }
    $it->fail='current'; echo iterator_count($it), "\n";
    attempt(function () use ($it) { iterator_apply($it, function () { throw new Exception('callback stopped'); }); });
    $it->fail=''; position($it);
    break;
case 'driver-contract':
    $it=new ArrayIterator([3,4]);
    foreach (['iterator_to_array','iterator_count'] as $name) {
        attempt(function () use ($name) { $name(); });
        attempt(function () use ($name) { $name(3); });
    }
    attempt(function () use ($it) { iterator_apply($it, 'missing_cursor_callback'); });
    attempt(function () use ($it) { iterator_apply($it, function () {}, 3); });
    echo iterator_count([5,6]), "\n";
    $count=0; echo iterator_apply($it,function () use (&$count) { $count++; return null; }), ':', $count, "\n";
    echo json_encode(iterator_to_array(iterator:new ArrayIterator(['v'=>4]), preserve_keys:false)), "\n";
    break;
case 'metadata':
    foreach (['rewind','current','key','next','valid','seek'] as $method) {
        $r=new ReflectionMethod('ArrayIterator',$method);
        echo $method, ':', $r->getNumberOfRequiredParameters(), '/', $r->getNumberOfParameters(), ':', $r->getTentativeReturnType(), "\n";
        foreach ($r->getParameters() as $p) echo $p->getName(), ':', $p->getType(), "\n";
    }
    echo (new ArrayIterator()) instanceof SeekableIterator ? "seekable\n" : "not seekable\n";
    foreach (['iterator_count','iterator_apply','iterator_to_array'] as $function) {
        $r=new ReflectionFunction($function);
        echo $function, ':', $r->getNumberOfRequiredParameters(), '/', $r->getNumberOfParameters(), ':', $r->getReturnType(), "\n";
        foreach ($r->getParameters() as $p) echo $p->getName(), ':', $p->getType(), ':', $p->isOptional() ? 'optional' : 'required', "\n";
    }
    break;
case 'callback-binding':
    $it=new ArrayIterator([1,2]); $n=3;
    var_dump(iterator_apply(iterator:$it,
        callback:function (&$total,$limit) { $total+=2; return $total<$limit; },
        args:['limit'=>6,'total'=>&$n]));
    var_dump($n);
    eval('declare(strict_types=1); try { iterator_to_array(new ArrayIterator([1]), 1); } catch (Throwable $e) { echo $e->getMessage(), "\\n"; }');
    break;
case 'projection-references':
    $value=6; $source=['ref'=>&$value,'plain'=>9];
    $it=new ArrayIterator($source);
    $copy=iterator_to_array($it); $copy['ref']=12; $copy['plain']=30;
    echo json_encode([$value,$source,$it->getArrayCopy(),$copy]), "\n"; position($it);
    $packed=iterator_to_array($it,false); $packed[0]=18;
    echo json_encode([$value,$packed]), "\n"; position($it);
    $plain=iterator_to_array($source); $plain['ref']=24;
    echo json_encode([$value,$plain]), "\n";
    $owner=new ArrayObject(['a'=>4,'b'=>7]); $view=$owner->getIterator(); $view->next();
    echo iterator_count($view), "\n"; position($view);
    $owner['c']=11; position($view);
    $empty=new ArrayIterator(); var_dump(iterator_to_array($empty),iterator_count($empty));
    $empty[]=20; position($empty);
    break;
case 'apply-release':
    class ApplyReleased {
        function __construct(public bool $fail) {}
        function __destruct() {
            echo 'drop|';
            if ($this->fail) throw new Exception('release');
        }
    }
    class ApplyPublicCursor extends ArrayIterator {
        function current(): mixed { echo 'unexpected-current|'; return parent::current(); }
    }
    foreach ([false,true] as $fail) {
        foreach ([false,true] as $public) {
            $it=$public ? new ApplyPublicCursor([new ApplyReleased($fail)]) : new ArrayIterator([new ApplyReleased($fail)]);
            try {
                $count=iterator_apply($it,function () use ($it) {
                    unset($it[0]); echo 'callback|'; return false;
                });
                echo "count=$count\n";
            } catch (Exception $error) { echo $error->getMessage(), "\n"; }
        }
    }
    break;
case 'method-projections':
    class ProjectionPayload {
        function __destruct() { echo 'drop|'; }
    }
    $payload=new ProjectionPayload();
    $it=new ArrayIterator(['heap'=>$payload,'tail'=>['v'=>7]]);
    unset($payload);
    var_dump($it->rewind(),$it->valid(),$it->key());
    $it->next();
    var_dump($it->key(),$it->valid());
    unset($it['heap']); echo "removed\n";
    $copy=$it->current(); $copy['v']=9;
    echo json_encode([$copy,$it->current()]), "\n";
    $it->next(); var_dump($it->valid(),$it->key(),$it->current());
    $it['late']=11; var_dump($it->valid(),$it->key(),$it->current());
    class ProjectionPublicCursor extends ArrayIterator {
        function current(): mixed { throw new Exception('unexpected-current'); }
        function key(): string|int|null { throw new Exception('unexpected-key'); }
    }
    $public=new ProjectionPublicCursor([['a'=>1],['b'=>2]]);
    var_dump(iterator_count($public));
    var_dump(iterator_apply($public,function () { return false; }));
    break;
case 'release-boundary':
    class CursorReleased {
        function __destruct() { echo 'drop|'; throw new Exception('released'); }
    }
    function cursorReleaseProbe($items) {
        $value=new CursorReleased(); $key='before';
        try { foreach ($items as $key=>$value) echo 'body|'; }
        catch (Exception $error) { echo $error->getMessage(), '|'; }
        var_dump($value,$key);
    }
    cursorReleaseProbe(['key'=>10]);
    cursorReleaseProbe(new ArrayIterator(['key'=>10]));
    cursorReleaseProbe(new ArrayObject(['key'=>10]));
    break;
}
