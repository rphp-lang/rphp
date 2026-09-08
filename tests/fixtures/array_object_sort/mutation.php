<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
foreach(['ArrayObject','ArrayIterator']as$class){foreach(['set','append','unset','ref','indirect','construct']as$operation){
    $o=new $class(['z'=>['n'=>3],'a'=>['n'=>1]]);$ran=false;
    $o->uasort(function($a,$b)use($o,&$ran,$operation){if(!$ran){$ran=true;try{switch($operation){
        case 'set':$o['x']=9;break;case 'append':$o->append(9);break;case 'unset':unset($o['z']);break;
        case 'ref':$r=&$o['z'];break;case 'indirect':$o['z']['n']=8;break;case 'construct':$o->__construct([]);break;
    }throw new Exception('mutation accepted');}catch(Error $e){same($e->getMessage(),'Modification of ArrayObject during sorting is prohibited');}
    same($o->getArrayCopy(),['z'=>['n'=>3],'a'=>['n'=>1]]);same($o['z']['n'],3);same(isset($o['z']),true);}
    return $a['n']<=>$b['n'];});same($ran,true);same(array_keys($o->getArrayCopy()),['a','z']);$o['later']=1;
}}
$o=new ArrayObject([3,1]);$o->uasort(function($a,$b)use($o){try{$o->exchangeArray([]);throw new Exception('exchange accepted');}catch(Error $e){same($e->getMessage(),'Modification of ArrayObject during sorting is prohibited');}return $a<=>$b;});
echo "mutation:ok\n";
