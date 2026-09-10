<?php
error_reporting(E_ALL);
set_error_handler(function ($kind, $message) { echo "diagnostic:$kind:$message\n"; return true; });
function show($value) { echo json_encode($value), "\n"; }
function attempt($callback) {
    try { show($callback()); } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
function traversal($mode, $depth) {
    $root = new RecursiveArrayIterator(['left'=>11, 'branch'=>['leaf'=>22, 'empty'=>[], 'deep'=>[33]], 'tail'=>44]);
    $walk = new RecursiveIteratorIterator($root, $mode);
    $walk->setMaxDepth($depth);
    $rows = [];
    foreach ($walk as $key=>$value) $rows[] = [$walk->getDepth(), $key, $value];
    show($rows);
    show([$walk->getDepth(), $walk->valid(), $walk->current(), $walk->key(), $walk->getSubIterator() === $root]);
}
switch (getenv('RPHP_RECURSIVE_CASE')) {
case 'modes':
    foreach ([0, 1, 2] as $mode) traversal($mode, -1);
    break;
case 'depth':
    foreach ([0, 1] as $mode) foreach ([0, 1] as $depth) traversal($mode, $depth);
    $walk = new RecursiveIteratorIterator(new RecursiveArrayIterator([1]));
    show($walk->getMaxDepth());
    $walk->setMaxDepth(3);
    attempt(fn()=>$walk->setMaxDepth(-2));
    show($walk->getMaxDepth());
    $walk->setMaxDepth(); show($walk->getMaxDepth());
    break;
case 'child-state':
    class Branch extends RecursiveArrayIterator {
        function __construct($data = [], $flags = 0) { echo "construct:$flags\n"; parent::__construct($data, $flags); }
    }
    $data = ['child'=>['value'=>7], 'empty'=>[], 'scalar'=>9];
    $root = new Branch($data, 2);
    $child = $root->getChildren();
    show([get_class($child), $child->getFlags(), $child->getArrayCopy()]);
    $child['value']=8;
    show([$data['child'], $root['child'], $child['value']]);
    $root->next(); show($root->hasChildren()); show($root->getChildren()->getArrayCopy());
    $root->next(); show($root->hasChildren()); attempt(fn()=>$root->getChildren());
    $root->next(); show($root->hasChildren()); attempt(fn()=>$root->getChildren());
    break;
case 'child-object':
    $nested = new RecursiveArrayIterator([71]);
    $root = new RecursiveArrayIterator([$nested, (object)['item'=>72]], 0);
    show([$root->hasChildren(), $root->getChildren() === $nested]);
    $root->setFlags(4); show($root->hasChildren());
    $root->next(); show($root->hasChildren());
    $root->setFlags(0); show($root->hasChildren());
    $child=$root->getChildren(); show($child->getArrayCopy());
    break;
case 'reference':
    $number=10; $data=['child'=>['alias'=>&$number]];
    $root=new RecursiveArrayIterator($data); $child=$root->getChildren();
    $child['alias']=20; show([$number,$data,$root['child']]);
    $walker=new RecursiveIteratorIterator($root);
    foreach($walker as $value) show($value);
    break;
case 'subiterator':
    $root=new RecursiveArrayIterator([[5]]); $walk=new RecursiveIteratorIterator($root);
    show([$walk->getDepth(),$walk->getSubIterator() === $root,$walk->getInnerIterator() === $root]);
    foreach([-2,-1,0,1,99,null] as $depth) show($walk->getSubIterator($depth) === $root);
    $walk->rewind();
    show([$walk->getDepth(),$walk->getSubIterator(0)===$root,$walk->getSubIterator()===$walk->getInnerIterator()]);
    foreach([-1,0,1,2] as $depth) show($walk->getSubIterator($depth)?->current());
    $walk->next(); show([$walk->getDepth(),$walk->getSubIterator()===$root]);
    break;
case 'uninitialized':
    class Unready extends RecursiveIteratorIterator { function __construct() {} }
    foreach(['rewind','valid','key','current','next','getDepth','getSubIterator','getInnerIterator','beginIteration','endIteration','callHasChildren','callGetChildren','beginChildren','endChildren','nextElement','getMaxDepth','setMaxDepth'] as $method) {
        echo "$method:"; attempt(fn()=>(new Unready())->$method());
    }
    attempt(function(){foreach(new Unready() as $value){};});
    attempt(fn()=>iterator_count(new Unready()));
    attempt(fn()=>iterator_to_array(new Unready()));
    break;
case 'constructor':
    foreach([[],new stdClass(),new ArrayIterator([1])] as $source) attempt(fn()=>new RecursiveIteratorIterator($source));
    class Source implements IteratorAggregate {
        function getIterator(): Traversable { echo "aggregate\n"; return new RecursiveArrayIterator([81]); }
    }
    $walk=new RecursiveIteratorIterator(new Source()); show(get_class($walk->getInnerIterator()));
    $other=new RecursiveArrayIterator([82]);
    attempt(function()use($walk,$other){$walk->__construct($other,1);return $walk->getInnerIterator()===$other;});
    foreach([-1,3,999] as $mode) attempt(function()use($other,$mode){return iterator_to_array(new RecursiveIteratorIterator($other,$mode));});
    break;
case 'hooks':
    class Steps extends RecursiveIteratorIterator {
        function beginIteration(): void { echo "begin\n"; }
        function endIteration(): void { echo "end\n"; }
        function beginChildren(): void { echo 'down:',$this->getDepth(),"\n"; }
        function endChildren(): void { echo 'up:',$this->getDepth(),"\n"; }
        function nextElement(): void { echo 'ready:',$this->getDepth(),':',$this->key(),"\n"; }
        function callHasChildren(): bool { $result=parent::callHasChildren(); echo 'has:',$this->getDepth(),':',(int)$result,"\n"; return $result; }
        function callGetChildren(): ?RecursiveIterator { echo "child\n"; return parent::callGetChildren(); }
    }
    $walk=new Steps(new RecursiveArrayIterator(['a'=>[1],'e'=>[],'z'=>2]));
    $walk->rewind(); show($walk->current()); $walk->rewind();
    while($walk->valid()) { show($walk->current()); $walk->next(); }
    show($walk->valid()); $walk->rewind(); show($walk->current());
    break;
case 'hook-errors':
    class FailingStep extends RecursiveIteratorIterator {
        public $failure = '';
        function fail($step) { if($this->failure===$step){$this->failure='';throw new RuntimeException($step);} }
        function callHasChildren():bool {$this->fail('has');return parent::callHasChildren();}
        function beginChildren():void {$this->fail('down');}
        function endChildren():void {$this->fail('up');}
        function nextElement():void {$this->fail('ready');}
    }
    foreach([0,16] as $flags) foreach(['has','down','up','ready'] as $step) {
        echo "$flags:$step\n";
        $walk=new FailingStep(new RecursiveArrayIterator([[5],6]),0,$flags);$walk->failure=$step;
        attempt(function()use($walk){$walk->rewind();while($walk->valid()){show($walk->current());$walk->next();}});
        show([$walk->getDepth(),$walk->current()]);
        attempt(function()use($walk){$walk->next();return $walk->current();});
    }
    break;
case 'child-exception':
    class Explosive extends RecursiveArrayIterator {
        function getChildren(): ?RecursiveArrayIterator { throw new RuntimeException('child blocked'); }
    }
    foreach([0,16] as $flags) foreach([0,1,2] as $mode) {
        echo "$flags:$mode\n";
        $walk=new RecursiveIteratorIterator(new Explosive(['a'=>[1],'z'=>2]),$mode,$flags);
        attempt(function()use($walk){$values=[];foreach($walk as $key=>$value)$values[$key]=$value;return $values;});
        show([$walk->getDepth(),$walk->key(),$walk->current()]);
    }
    break;
case 'override':
    class Forks extends RecursiveArrayIterator {
        function hasChildren(): bool { echo 'has:',$this->key(),"\n"; return parent::hasChildren(); }
        function getChildren(): ?RecursiveArrayIterator { echo 'get:',$this->key(),"\n"; return new self($this->current()); }
        function key(): string|int|null { return 'k'.parent::key(); }
        function marker($value) { return $value+4; }
    }
    $walk=new RecursiveIteratorIterator(new Forks([[1],2]));
    show($walk->marker(3)); foreach($walk as $key=>$value) show([$key,$value]);
    break;
case 'reflection':
    foreach(['RecursiveIterator','RecursiveArrayIterator','RecursiveIteratorIterator'] as $class) {
        $reflection=new ReflectionClass($class); echo "$class\n";
        foreach($reflection->getMethods() as $method) {
            if($method->getDeclaringClass()->name !== $class)continue;
            $parameters=[];
            foreach($method->getParameters() as $parameter)$parameters[]=[$parameter->getName(),(string)$parameter->getType(),$parameter->isOptional()];
            show([$method->name,$parameters,(string)$method->getTentativeReturnType()]);
        }
    }
    break;
case 'reentry':
    class Reentered extends RecursiveIteratorIterator {
        public $once=true;
        function nextElement():void {
            echo 'ready:',$this->current(),"\n";
            if($this->once){$this->once=false;$this->next();show($this->current());}
        }
    }
    $walk=new Reentered(new RecursiveArrayIterator([[4,5],6]));
    foreach($walk as $value)show($value);
    class Replaced extends RecursiveIteratorIterator {
        public $once=true;
        function beginChildren():void {
            if($this->once){$this->once=false;$this->__construct(new RecursiveArrayIterator([8,9]));}
        }
    }
    $walk=new Replaced(new RecursiveArrayIterator([[1,2],3]));
    $walk->rewind();for($n=0;$n<5&&$walk->valid();$n++,$walk->next())show($walk->current());
    break;
case 'advance-error':
    class FailingAdvance extends RecursiveArrayIterator {
        public $once=true;
        function next():void {if($this->once){$this->once=false;throw new RuntimeException('advance');}parent::next();}
    }
    foreach([0,16] as $flags){
        $walk=new RecursiveIteratorIterator(new FailingAdvance([3,4]),0,$flags);
        $walk->rewind(); attempt(fn()=>$walk->next()); show($walk->current());
        $walk->next();show($walk->current());
    }
    break;
case 'retirement':
    class OwnedBranch extends RecursiveArrayIterator {
        public string $label;
        function __construct(array $items, string $label) {
            parent::__construct($items);
            $this->label = $label;
        }
        function getChildren(): ?RecursiveArrayIterator {
            return new self($this->current(), 'child');
        }
        function __destruct() { echo 'drop:', $this->label, "\n"; }
    }
    class ReplacingDriver extends RecursiveIteratorIterator {
        public bool $once = true;
        function beginChildren(): void {
            if ($this->once) {
                $this->once = false;
                echo "begin-replace\n";
                $this->__construct(new OwnedBranch([9], 'new'));
                echo "end-replace\n";
            }
        }
    }
    $driver = new ReplacingDriver(new OwnedBranch([[1]], 'old'));
    $driver->rewind();
    echo 'after:', $driver->current(), "\n";
    unset($driver);
    echo "done\n";
    $held = new OwnedBranch([[2]], 'held-old');
    $driver = new ReplacingDriver($held);
    $driver->rewind();
    echo "held-alive\n";
    unset($driver);
    echo "release-held\n";
    unset($held);
    echo "alias-done\n";
    break;
case 'argument-reference-proof':
    require __DIR__.'/argument-reference-proof.php';
    break;
case 'scalar-evaluation-proof':
    require __DIR__.'/scalar-evaluation-proof.php';
    break;
case 'property-reference-proof':
    require __DIR__.'/property-reference-proof.php';
    break;
case 'invalid-child-retirement':
    class RejectedBranch {
        function __construct(public bool $throws) {}
        function __destruct() {
            echo "drop-invalid\n";
            if ($this->throws) throw new RuntimeException('drop-error');
        }
    }
    class RejectingDriver extends RecursiveIteratorIterator {
        public string $behavior = '';
        public $held;
        #[ReturnTypeWillChange]
        function callGetChildren() {
            echo "make-invalid\n";
            $child = new RejectedBranch($this->behavior === 'throw');
            if ($this->behavior === 'held') $this->held = $child;
            return $child;
        }
    }
    foreach ([0, 16] as $flags) foreach (['normal', 'throw', 'held'] as $behavior) {
        echo "$flags:$behavior\n";
        $driver = new RejectingDriver(new RecursiveArrayIterator([[17]]), 0, $flags);
        $driver->behavior = $behavior;
        try { $driver->rewind(); }
        catch (Throwable $error) {
            echo get_class($error), ':', $error->getMessage(), "\n";
            $previous = $error->getPrevious();
            echo 'previous:', $previous ? get_class($previous) : 'none', "\n";
        }
        echo "after\n";
        unset($driver);
        echo "done\n";
    }
    break;
case 'replace-during-unwind':
    class ResetAtEnd extends RecursiveIteratorIterator {
        public $once=true;
        function endChildren():void {
            if($this->once){$this->once=false;echo "replace\n";$this->__construct(new RecursiveArrayIterator([8,9]));}
        }
    }
    $walk=new ResetAtEnd(new RecursiveArrayIterator([[1],2]));
    $walk->rewind();for($n=0;$n<6&&$walk->valid();$n++,$walk->next())show([$walk->current(),$walk->getDepth()]);
    break;
}
