<?php
function report($label, $action) {
    try { $result = $action(); echo $label, '=', serialize($result), "\n"; }
    catch (Throwable $error) { echo $label, '!', get_class($error), ':', $error->getMessage(), "\n"; }
}
function contents($heap) {
    $result = [];
    while (!$heap->isEmpty()) $result[] = $heap->extract();
    return $result;
}
set_error_handler(function ($level, $message) { echo "diag:$level:$message\n"; return true; });
class StoredHeap extends SplMaxHeap {
    public $label = 'old';
    public int $typed = 17;
    public readonly int $fixed;
    protected $hidden = 'protected';
    private $privateSlot = 'private';
    public function __construct() { $this->fixed = 19; }
}
class RestoringHeap extends SplMaxHeap {
    public $action = '';
    public function insert($value): true { echo "override-insert\n"; return parent::insert($value); }
    public function compare($a, $b): int {
        if ($this->action === 'throw') throw new RuntimeException('comparison-stop');
        if ($this->action) {
            $action = $this->action; $this->action = '';
            report('callback-'.$action, fn() => $action === 'save' ? $this->__serialize() : $this->__unserialize([]));
        }
        return parent::compare($a, $b);
    }
}
switch (getenv('RPHP_CONTAINER_SERIALIZATION_CASE')) {
case 'collected-cycles':
    foreach (['SplMaxHeap','SplDoublyLinkedList','SplPriorityQueue'] as $class) {
        $box=new $class;$value=new stdClass;$value->owner=$box;
        if ($box instanceof SplPriorityQueue) $box->__unserialize([[],['flags'=>1,'heap_elements'=>[['data'=>&$value,'priority'=>1]]]]);
        elseif ($box instanceof SplHeap) $box->__unserialize([[],['flags'=>0,'heap_elements'=>[&$value]]]);
        else $box->__unserialize([0,[&$value],[]]);
        $wire=serialize($box);$weak=WeakReference::create($box);unset($box,$value);gc_collect_cycles();
        var_dump($weak->get()===null);
        $copy=unserialize($wire);$item=$copy->top();var_dump($item->owner===$copy);
        $weak=WeakReference::create($copy);unset($item,$copy);gc_collect_cycles();var_dump($weak->get()===null);
    }
    break;
case 'clone-references':
    foreach (['SplMaxHeap','SplDoublyLinkedList'] as $class) {
        $value=6;$box=new $class;
        $box->__unserialize($box instanceof SplHeap?[[],['flags'=>0,'heap_elements'=>[&$value]]]:[0,[&$value],[]]);
        $copy=clone $box;$value=12;
        report('cloned',fn()=>[$box->top(),$copy->top()]);
        $state=$copy->__serialize();
        if ($box instanceof SplHeap) $state[1]['heap_elements'][0]=18;
        else $state[1][0]=18;
        report('write-through',fn()=>[$value,$box->top(),$copy->top()]);
    }
    break;
case 'member-retirement':
    class SlotHeap extends SplMaxHeap {public $slot;}
    class SlotList extends SplDoublyLinkedList {public $slot;}
    class MemberRetired {public function __destruct(){global $box;echo 'drop:',gettype($box->slot),':',$box->count(),"\n";}}
    foreach (['SlotHeap','SlotList'] as $class) {
        $box=new $class;$box->slot=new MemberRetired;
        if ($box instanceof SplHeap) {$box->insert(1);$data=[['slot'=>'new'],['flags'=>0,'heap_elements'=>[3]]];}
        else {$box->push(1);$data=[0,[3],['slot'=>'new']];}
        report('restore',fn()=>$box->__unserialize($data));
        report('after',fn()=>[$box->slot,$box->count()]);
    }
    break;
case 'member-errors':
    class ErrorHeap extends SplMaxHeap {public $slot;}
    class ErrorList extends SplDoublyLinkedList {public $slot;}
    class ThrowMember {public function __destruct(){throw new RuntimeException('retirement-stop');}}
    foreach (['ErrorHeap','ErrorList'] as $class) {
        $box=new $class;$value=3;$box->slot=&$value;
        $data=$box instanceof SplHeap?[['slot'=>5],['flags'=>0,'heap_elements'=>[]]]:[0,[],['slot'=>5]];
        $box->__unserialize($data);report('detached',fn()=>[$value,$box->slot]);
        $box->slot=new ThrowMember;
        report('retire',fn()=>$box->__unserialize($data));report('slot',fn()=>$box->slot);
        set_error_handler(function(){global $box;echo 'inside:',serialize(get_object_vars($box)),"\n";throw new RuntimeException('diagnostic-stop');});
        $data=$box instanceof SplHeap?[['fresh'=>6],['flags'=>0,'heap_elements'=>[]]]:[0,[],['fresh'=>6]];
        report('diagnostic',fn()=>$box->__unserialize($data));restore_error_handler();
        report('properties',fn()=>get_object_vars($box));
    }
    break;
case 'override-hooks':
    class HookHeap extends SplMaxHeap {
        public function serialize():string {echo "legacy-called\n";return '';}
        public function __serialize():array {echo "save-hook\n";return parent::__serialize();}
        public function __unserialize(array $data):void {echo "load-hook\n";parent::__unserialize($data);}
    }
    $heap=new HookHeap;$heap->insert(4);$copy=unserialize(serialize($heap));report('loaded',fn()=>$copy->top());
    class HookList extends SplDoublyLinkedList {
        public function __serialize():array {echo "save-list\n";return parent::__serialize();}
        public function __unserialize(array $data):void {echo "load-list\n";parent::__unserialize($data);}
    }
    $box=new HookList;$box->push(5);$copy=unserialize(serialize($box));report('loaded-list',fn()=>$copy->top());
    break;
case 'legacy-roundtrip':
    foreach (['SplDoublyLinkedList','SplStack','SplQueue'] as $class) {
        $box=new $class;$box->push('first');$box->push(7);$box->rewind();
        report('interface',fn()=>$box instanceof Serializable);
        $wire=$box->serialize();report('wire',fn()=>$wire);
        report('restore',fn()=>$box->unserialize($wire));
        report('after',fn()=>[$box->__serialize(),$box->current(),$box->valid()]);
        report('repeat',fn()=>$box->unserialize($wire));
        report('count',fn()=>$box->count());
    }
    break;
case 'legacy-validation':
    foreach (['','bad','i:0;','i:0;:i:4;','i:0;:i:4;tail','i:0;:i:4;:b','b:1;','s:1:"4";','i:0;:','i:0;:b:2;','i:0;:i:5;:R:2;','i:0;:i:5;:R:99;'] as $wire) {
        $box=new SplDoublyLinkedList;$box->push(2);$box->rewind();
        report('load',fn()=>$box->unserialize($wire));
        report('after',fn()=>[$box->__serialize(),$box->current(),$box->key(),$box->valid()]);
    }
    break;
case 'legacy-graph':
    $payload='i:1;:i:27;';
    $wire='a:3:{i:0;C:19:"SplDoublyLinkedList":'.strlen($payload).':{'.$payload.'}i:1;R:3;i:2;R:4;}';
    $graph=unserialize($wire);report('initial',fn()=>[$graph[0]->__serialize(),$graph[1],$graph[2]]);
    $graph[1]=8;$graph[2]=35;
    report('aliases',fn()=>[$graph[0]->getIteratorMode(),$graph[0]->top()]);
    $box=new SplStack;$value=9;$box->__unserialize([6,[&$value,&$value],[]]);
    $wire=$box->serialize();report('wire',fn()=>$wire);
    $copy=new SplStack;$copy->unserialize($wire);$state=$copy->__serialize();$state[1][0]=45;
    report('repeat-reference',fn()=>$copy->__serialize());
    break;
case 'legacy-callbacks':
    class CallbackItem {
        public static $action='';
        public function __serialize():array {
            global $box; echo 'callback:',$box->count(),"\n";
            if (self::$action==='head') $box->shift();
            elseif (self::$action==='tail') $box->pop();
            else {$box->push(8);self::$action='tail';}
            return [];
        }
    }
    foreach (['head','tail','append'] as $action) {
        $box=new SplDoublyLinkedList;$box->push(new CallbackItem);$box->push(2);CallbackItem::$action=$action;
        report('wire',fn()=>$box->serialize());report('remaining',fn()=>$box->count());
    }
    class SelfRemovingItem {
        public function __serialize():array {global $box;echo "self-hook\n";$box->shift();return [];}
        public function __destruct(){global $box;echo 'self-drop:',$box->count(),"\n";}
    }
    $box=new SplDoublyLinkedList;$box->push(new SelfRemovingItem);$box->push(7);
    report('self-removing',fn()=>$box->serialize());
    break;
case 'legacy-retirement':
    class RetiredItem {
        public $label;
        public function __construct($label){$this->label=$label;}
        public function __destruct(){global $box;echo 'retire:',$this->label,':',$box->count(),':',$box->getIteratorMode(),"\n";}
    }
    $box=new SplDoublyLinkedList;$box->setIteratorMode(2);
    $box->push(new RetiredItem('first'));$box->push(new RetiredItem('second'));$box->rewind();
    report('load',fn()=>$box->unserialize('i:3;:i:6;'));
    report('state',fn()=>$box->__serialize());
    break;
case 'deque-projection':
    foreach (['SplDoublyLinkedList','SplStack','SplQueue'] as $class) {
        $box=new $class;
        report('empty',fn()=>$box->__serialize());
        $box->push('left');$box->push('right');$box->rewind();
        report('state',fn()=>$box->__serialize());
        report('wire',fn()=>serialize($box));
        $copy=unserialize(serialize($box));
        report('restore',fn()=>[$copy->__serialize(),$copy->current(),$copy->valid()]);
        report('original',fn()=>[$box->count(),$box->current(),$box->key()]);
    }
    break;
case 'deque-validation':
    foreach ([[],[0,[],[],9],[1=>0,2=>[],3=>[]],['0',[],[]],[3,null,[]],[3,[9],null],[0,[9]]] as $data) {
        $box=new SplStack;$box->push(4);$box->rewind();
        report('load',fn()=>$box->__unserialize($data));
        report('after',fn()=>[$box->__serialize(),$box->current(),$box->key()]);
    }
    foreach ([-1,0,2,7,256,2147483648,4294967296,PHP_INT_MAX] as $flags) {
        $box=new SplQueue;$box->push(4);$box->rewind();
        report('load',fn()=>$box->__unserialize([$flags,['x'=>9,7=>3],[]]));
        report('state',fn()=>[$box->__serialize(),$box->getIteratorMode(),$box->current()]);
    }
    break;
case 'deque-members':
    class StoredList extends SplDoublyLinkedList {
        public $label='old'; public int $typed=3; public readonly int $fixed;
        public function __construct(){$this->fixed=8;}
    }
    $box=new StoredList;$box->push(2);
    report('load',fn()=>$box->__unserialize([3,[6],['typed'=>'raw','label'=>'changed']]));
    report('state',fn()=>$box->__serialize());
    report('readonly',fn()=>$box->__unserialize([0,[9],['label'=>'partial','fixed'=>7,'typed'=>99]]));
    report('partial',fn()=>$box->__serialize());
    break;
case 'deque-references':
    $v=4;$box=new SplDoublyLinkedList;$box->__unserialize([0,[&$v],[]]);
    $v=7;report('input',fn()=>$box->top());
    $state=$box->__serialize();$state[1][0]=11;report('output',fn()=>[$v,$box->top()]);
    $graph=unserialize(serialize([$box,&$v]));$graph[1]=15;report('wire',fn()=>$graph[0]->top());
    $box=new SplQueue;$box->push($box);$copy=unserialize(serialize($box));
    report('self',fn()=>$copy->top()===$copy);$box->pop();$copy->pop();
    break;
case 'projection':
    foreach (['SplMaxHeap', 'SplMinHeap', 'SplPriorityQueue'] as $class) {
        $heap = new $class;
        report('empty', fn() => $heap->__serialize());
        foreach ([7, 1, 12, 4] as $value) {
            if ($heap instanceof SplPriorityQueue) $heap->insert('item'.$value, $value);
            else $heap->insert($value);
        }
        if ($heap instanceof SplPriorityQueue) $heap->setExtractFlags(3);
        report('projection', fn() => $heap->__serialize());
        report('wire', fn() => serialize($heap));
        $copy = unserialize(serialize($heap));
        report('restored', fn() => [get_class($copy), $copy->count(), contents($copy)]);
        report('original', fn() => $heap->count());
    }
    break;
case 'partial-validation':
    foreach ([[], [[],[],3], [2=>[],3=>[]], [null,[]], [['label'=>'new'],null], [['label'=>'new'],[]], [['label'=>'new'],['flags'=>0]], [['label'=>'new'],['flags'=>0,'heap_elements'=>false]]] as $data) {
        $heap = new StoredHeap; $heap->insert(9);
        report('restore', fn() => $heap->__unserialize($data));
        report('after', fn() => [$heap->label, $heap->count(), $heap->current()]);
    }
    break;
case 'flags':
    foreach ([new SplMaxHeap, new SplPriorityQueue] as $heap) {
        foreach ([-1,0,1,2,3,4,5,7,256,257,PHP_INT_MAX,'1',true,null] as $flags) {
            report('load', fn() => $heap->__unserialize([[], ['flags'=>$flags,'heap_elements'=>[]]]));
            report('state', fn() => $heap->__serialize());
        }
    }
    break;
case 'append-restore':
    $heap = new RestoringHeap; $heap->insert(5);
    report('load', fn() => $heap->__unserialize([[], ['flags'=>0,'heap_elements'=>['named'=>2,9=>11,3=>7]]]));
    report('drain', fn() => contents($heap));
    $queue = new SplPriorityQueue; $queue->insert('old',6);
    report('partial', fn() => $queue->__unserialize([[], ['flags'=>3,'heap_elements'=>[['priority'=>12,'data'=>'first'],['data'=>'bad']]]]));
    report('state', fn() => $queue->__serialize());
    report('extra-field', fn() => $queue->__unserialize([[], ['flags'=>1,'heap_elements'=>[['data'=>'no','priority'=>50,'extra'=>true]]]]));
    report('remaining', fn() => contents($queue));
    break;
case 'members':
    $heap = new StoredHeap; $heap->insert(10);
    report('load', fn() => $heap->__unserialize([['label'=>'replaced','typed'=>'raw'], ['flags'=>0,'heap_elements'=>[3]]]));
    report('members', fn() => $heap->__serialize());
    report('readonly', fn() => $heap->__unserialize([['label'=>'committed','fixed'=>20,'typed'=>8], ['flags'=>0,'heap_elements'=>[99]]]));
    report('after', fn() => $heap->__serialize());
    $heap = new StoredHeap; $heap->insert(4);
    $copy = unserialize(serialize($heap));
    report('roundtrip', fn() => $copy->__serialize());
    break;
case 'references':
    $value=8; $heap=new SplMaxHeap;
    $heap->__unserialize([[],['flags'=>0,'heap_elements'=>[&$value]]]);
    $value=14; report('input-alias',fn()=>$heap->current());
    $data=$heap->__serialize(); $data[1]['heap_elements'][0]=21;
    report('output-alias',fn()=>[$value,$heap->current()]);
    $array=['x'=>3]; $ordinary=new SplMaxHeap; $ordinary->insert($array);
    $data=$ordinary->__serialize(); $data[1]['heap_elements'][0]['x']=9;
    report('cow',fn()=>[$array,$ordinary->current()]);
    $data=unserialize(serialize([$heap,&$value]));$data[1]=34;
    report('wire-alias',fn()=>$data[0]->current());
    break;
case 'identity-cycles':
    $heap=new SplPriorityQueue; $shared=new stdClass; $shared->label='shared';
    $heap->insert($shared,2);$heap->insert($shared,1);
    $copy=unserialize(serialize([$heap,$shared]));
    report('identity',fn()=>[$copy[0]->extract()===$copy[1],$copy[0]->extract()===$copy[1]]);
    $heap=new SplMaxHeap;$heap->insert($heap);$copy=unserialize(serialize($heap));
    report('cycle',fn()=>$copy->current()===$copy);
    $heap->extract();$copy->extract();
    break;
case 'locks':
    foreach (['save','load'] as $action) {
        $heap=new RestoringHeap;$heap->insert(4);$heap->action=$action;
        report('insert',fn()=>$heap->insert(9));
        report('state',fn()=>[$heap->count(),$heap->isCorrupted()]);
    }
    $heap=new RestoringHeap;$heap->insert(4);$heap->action='throw';
    report('insert',fn()=>$heap->insert(9));
    report('save',fn()=>$heap->__serialize());
    report('load',fn()=>$heap->__unserialize([]));
    $heap->action='';$heap->recoverFromCorruption();
    report('recovered',fn()=>$heap->__serialize());
    break;
case 'metadata':
    foreach (['SplHeap','SplMaxHeap','SplPriorityQueue'] as $class) foreach (['__serialize','__unserialize'] as $name) {
        $method=new ReflectionMethod($class,$name);
        report('signature',fn()=>[$method->getDeclaringClass()->getName(),$method->getName(),$method->getNumberOfRequiredParameters(),(string)$method->getTentativeReturnType(),array_map(fn($p)=>[$p->getName(),(string)$p->getType()],$method->getParameters())]);
    }
    $heap=new SplMinHeap;
    report('named',fn()=>$heap->__unserialize(data:[[],['flags'=>0,'heap_elements'=>[6]]]));
    report('wrong-type',fn()=>$heap->__unserialize('bad'));
    report('wrong-name',fn()=>$heap->__unserialize(state:[]));
    report('missing',fn()=>$heap->__unserialize());
    break;
}
