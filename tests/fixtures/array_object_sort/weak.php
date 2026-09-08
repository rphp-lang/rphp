<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
$warnings=[];set_error_handler(function($level,$message)use(&$warnings){$warnings[]=$message;return true;});
foreach(['ArrayObject','ArrayIterator']as$class){$o=new $class(['b'=>'12','a'=>'2']);same($o->asort(flags:'1'),true);same(array_keys($o->getArrayCopy()),['a','b']);
    same($o->ksort(flags:null),true);same(array_keys($o->getArrayCopy()),['a','b']);same($warnings[count($warnings)-1],$class.'::ksort(): Passing null to parameter #1 ($flags) of type int is deprecated');
    foreach([[],new stdClass(),'not-flags']as$value){try{$o->asort($value);throw new Exception('invalid weak flag');}catch(TypeError $e){}}
    $p=(new ReflectionMethod($class,'asort'))->getParameters()[0];same($p->getDefaultValueConstantName(),'SORT_REGULAR');
}
restore_error_handler();same(count($warnings),2);
echo "weak:ok\n";
