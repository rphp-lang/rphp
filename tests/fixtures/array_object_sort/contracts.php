<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
foreach(['ArrayObject','ArrayIterator']as$class){foreach(['asort','ksort','natsort','natcasesort','uasort','uksort']as$method){
    $r=new ReflectionMethod($class,$method);$callback=$method==='uasort'||$method==='uksort';$flags=$method==='asort'||$method==='ksort';
    same($r->getNumberOfRequiredParameters(),$callback?1:0);same($r->getNumberOfParameters(),$callback||$flags?1:0);
    same((string)$r->getTentativeReturnType(),'true');
    foreach($r->getParameters()as$p){same($p->getName(),$callback?'callback':'flags');same((string)$p->getType(),$callback?'callable':'int');if($flags)same($p->getDefaultValue(),0);}
    $o=new $class([2,1]);try{$o->$method(1,2);throw new Exception('accepted excess args');}catch(ArgumentCountError $e){}
    if($callback){try{$o->$method();throw new Exception('accepted absent callback');}catch(ArgumentCountError $e){}
        try{$o->$method('missing_array_compare');throw new Exception('accepted invalid callback');}catch(TypeError $e){same($e->getMessage(),$method.'(): Argument #2 ($callback) must be a valid callback, function "missing_array_compare" not found or invalid function name');}}
}}
echo "contracts:ok\n";
