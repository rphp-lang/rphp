<?php
class SideInvoke { public function __invoke($value){return "i:".$value;} }
class SideMagic { public function __call($name,$args){return $name."(".implode(",",$args).")";} }
class SideA { public static function scope($value){return static::class.":".$value;} }
class SideB extends SideA {}
$invoke=new SideInvoke(); $magic=new SideMagic();
for($i=0;$i<6;$i++){echo $magic->missing($invoke(SideB::scope($i)), $invoke("x")),"|";}
$closure=function($value){static $n=0;return ++$n.":".$value;};
echo $closure($invoke("c")),"|",$closure($magic->other("z")),"|";
try {$magic->missing((function(){throw new Exception("x");})());}catch(Exception $e){echo "caught|";}
echo $magic->after(SideB::scope("done")),"|";
