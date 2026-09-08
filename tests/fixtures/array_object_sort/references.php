<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
$v=4;$source=['b'=>&$v,'a'=>1];$o=new ArrayObject($source);$copy=$o->getArrayCopy();$o->asort();
same(array_keys($source),['b','a']);same(array_keys($copy),['b','a']);same(array_keys($o->getArrayCopy()),['a','b']);
$v=9;same($o['b'],9);$o['a']=7;same($source['a'],1);
set_error_handler(fn()=>true);$root=new ArrayObject(['b'=>2,'a'=>1]);$view=new ArrayObject($root);restore_error_handler();
$once=false;$root->uasort(function($a,$b)use($view,&$once){if(!$once){$once=true;$view['temporary']=8;}return $a<=>$b;});
same($root->getArrayCopy(),['a'=>1,'b'=>2]);same($view->getArrayCopy(),['a'=>1,'b'=>2]);
$view->ksort();same($root->getArrayCopy(),$view->getArrayCopy());
echo "references:ok\n";
