<?php
$globalCounter=0;
function dynamicFrame($id){global $globalCounter; $globalCounter++; $name="v".$id; $$name=$id; $vars=get_defined_vars(); return $vars[$name].":".$globalCounter;}
for($i=0;$i<5;$i++){echo dynamicFrame($i),"|";}
$fn=function(){static $n=0; $name="dynamic"; $$name=++$n; return get_defined_vars()[$name];};
echo $fn(),"|",$fn(),"|",dynamicFrame(9),"|";
class FrameScope { public static function name(){return static::class;} }
class FrameChild extends FrameScope {}
$fiber=new Fiber(function(){echo FrameChild::name(),"|"; Fiber::suspend(dynamicFrame(10)); echo FrameChild::name(),"|"; return dynamicFrame(11);});
echo $fiber->start(),"|"; $fiber->resume(); echo $fiber->getReturn(),"|",dynamicFrame(12),"|";
