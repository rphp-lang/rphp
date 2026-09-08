<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
foreach(['ArrayObject','ArrayIterator']as$class){
    $source=['last'=>9,'first'=>1,'equal'=>1,'middle'=>4];$o=new $class($source);
    same($o->asort(),true);same($o->getArrayCopy(),['first'=>1,'equal'=>1,'middle'=>4,'last'=>9]);same($source['last'],9);
    same($o->ksort(flags:SORT_STRING),true);same(array_keys($o->getArrayCopy()),['equal','first','last','middle']);
    $o=new $class(['x'=>'item12','y'=>'item2','z'=>'ITEM3']);same($o->natsort(),true);
    same(array_keys($o->getArrayCopy()),['z','y','x']);same($o->natcasesort(),true);same(array_keys($o->getArrayCopy()),['y','z','x']);
    $o=new $class(['low'=>1,'high'=>7,'mid'=>4]);same($o->uasort(callback:fn($a,$b)=>$b<=>$a),true);
    same(array_keys($o->getArrayCopy()),['high','mid','low']);same($o->uksort(callback:fn($a,$b)=>strcmp($a,$b)),true);
    same(array_keys($o->getArrayCopy()),['high','low','mid']);
    $o=new $class();same($o->asort(),true);same($o->uasort(fn($a,$b)=>0),true);same($o->getArrayCopy(),[]);
}
echo "basic:ok\n";
