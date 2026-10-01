<?php
class PendingOwner {
 public function __invoke($value){echo "invoke:",$value,"|";}
 public function __destruct(){echo "drop|";}
}
$call=new PendingOwner(); $weak=WeakReference::create($call);
$fiber=new Fiber(function() use(&$call){$call(Fiber::suspend("paused"));});
echo $fiber->start(),"|"; $call=null;
gc_collect_cycles(); echo $weak->get()!==null?"held|":"lost|";
$fiber->resume("resumed"); gc_collect_cycles();
echo $weak->get()===null?"gone|":"live|";
