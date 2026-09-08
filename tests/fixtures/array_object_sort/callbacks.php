<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
$expected=[1=>['a','b','c','d','e'],2=>['b','a','c','d','e'],3=>['b','a','c','d','e'],5=>['b','c','d','a','e']];
foreach($expected as$at=>$keys){$o=new ArrayObject(['a'=>5,'b'=>3,'c'=>4,'d'=>1,'e'=>2]);$calls=0;
    try{$o->uasort(function($a,$b)use(&$calls,$at){if(++$calls===$at)throw new Exception('comparison-stop');return $a<=>$b;});throw new Exception('missing callback exception');}
    catch(Exception $e){same($e->getMessage(),'comparison-stop');}
    same(array_keys($o->getArrayCopy()),$keys);$o['later']=0;unset($o['later']);same($o->asort(),true);
}
$warnings=[];set_error_handler(function($level,$message)use(&$warnings){$warnings[]=$message;return true;});
$o=new ArrayObject(['x'=>3,'y'=>1,'z'=>2]);same($o->uasort(fn($a,$b)=>$a>$b),true);restore_error_handler();
same(count($warnings),1);same($warnings[0],'uasort(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero');same(array_keys($o->getArrayCopy()),['y','z','x']);
$o=new ArrayObject([4,1,3]);$once=false;$o->uasort(function($a,$b)use($o,&$once){if(!$once){$once=true;$o->ksort();}return $a<=>$b;});same($o->getArrayCopy(),[1=>1,2=>3,0=>4]);
echo "callbacks:ok\n";
