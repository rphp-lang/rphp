<?php
declare(strict_types=1);
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
foreach(['ArrayObject','ArrayIterator']as$class){foreach(['asort','ksort']as$method){$o=new $class(['z'=>2,'a'=>1]);
    foreach([null,true,2.0,'2',[],new stdClass()]as$arg){try{$o->$method($arg);throw new Exception('accepted strict flag');}catch(TypeError $e){same(str_starts_with($e->getMessage(),$class.'::'.$method.'(): Argument #1 ($flags) must be of type int,'),true);}}
    same($o->getArrayCopy(),['z'=>2,'a'=>1]);same($o->$method(2),true);
}}
echo "strict:ok\n";
