<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
foreach([2,3,5,6,16,17,33,64]as$n){$source=[];for($i=0;$i<$n;$i++)$source['k'.$i]=($n-$i)%7;
    foreach(['asort','ksort','natsort','natcasesort','uasort','uksort']as$method){$expected=$source;
        if($method==='uasort'||$method==='uksort'){$callback=fn($a,$b)=>$a<=>$b;$method($expected,$callback);$o=new ArrayObject($source);$o->$method($callback);}
        else{$method($expected);$o=new ArrayObject($source);$o->$method();}
        same($o->getArrayCopy(),$expected);
    }
}
echo "boundaries:ok\n";
