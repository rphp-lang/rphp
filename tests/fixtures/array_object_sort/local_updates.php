<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
function steps($value){
    $a=++$value;$b=$value++;$c=--$value;$d=$value--;
    return [$a,$b,$c,$d,$value];
}
foreach([-31,0,47,PHP_INT_MAX-2,PHP_INT_MIN+2]as$seed){
    same(steps($seed),[$seed+1,$seed+1,$seed+1,$seed+1,$seed]);
}
function discarded($value){++$value;$value++;--$value;$value--;return $value;}
same(discarded(71),71);
$top=PHP_INT_MAX;$previous=$top++;same($previous,PHP_INT_MAX);same(is_float($top),true);
$bottom=PHP_INT_MIN;$previous=$bottom--;same($previous,PHP_INT_MIN);same(is_float($bottom),true);
$top=PHP_INT_MAX;same(is_float(++$top),true);
$bottom=PHP_INT_MIN;same(is_float(--$bottom),true);
$number=12;$alias=&$number;$a=$alias++;$b=++$alias;$c=$alias--;$d=--$alias;
same([$a,$b,$c,$d,$number],[12,14,14,12,12]);
class UpdateCell{public int $number=6;}
$cell=new UpdateCell;$alias=&$cell->number;
same([$alias++,++$alias,$alias--,--$alias,$cell->number],[6,8,8,6,6]);
$cell->number=PHP_INT_MAX;try{$alias++;throw new Exception('typed overflow accepted');}
catch(TypeError $error){same($cell->number,PHP_INT_MAX);}
unset($alias);
$shared=9;function global_steps(){global $shared;$first=$shared++;++$shared;--$shared;return [$first,$shared--];}
same(global_steps(),[9,10]);same($shared,9);
$events=[];set_error_handler(function($level,$message)use(&$events){$events[]=$level;return true;});
$flag=true;same($flag++,true);same(--$flag,true);
$missing=null;same($missing--,null);same(++$missing,1);
$text='part8';same($text++,'part8');same($text,'part9');
restore_error_handler();same(count($events),4);
same(steps(2.5),[3.5,3.5,3.5,3.5,2.5]);
$text='099';same(++$text,100);same($text--,100);same($text,99);
$array=[];try{$array++;throw new Exception('array increment accepted');}
catch(TypeError $error){same($array,[]);}
$object=new ArrayObject(['value'=>4]);same([$object['value']++,++$object['value'],$object['value']--,--$object['value']],[4,6,6,4]);
function local_order($value){return [$value++,++$value,$value--,--$value,$value];}
same(local_order(18),[18,20,20,18,18]);
function observation_barrier(){}
$snapshot=17;observation_barrier();$a=$snapshot++;
observation_barrier();$b=++$snapshot;observation_barrier();$c=$snapshot--;
observation_barrier();$d=--$snapshot;same([$a,$b,$c,$d,$snapshot],[17,19,19,17,17]);
$snapshot=4;function replace_snapshot(){$GLOBALS['snapshot']=21;}
replace_snapshot();same([$snapshot++,++$snapshot,$snapshot],[21,23,23]);
echo "local_updates:ok\n";
