<?php
function same($a,$b){if($a!==$b)throw new Exception(json_encode([$a,$b]));}
class SortText {
    public static $owner;
    public static $calls=0;
    public static $drops=[];
    function __construct(public $text){}
    function __toString(){++self::$calls;try{self::$owner->append(8);throw new Exception('allowed comparator write');}catch(Error $e){same($e->getMessage(),'Modification of ArrayObject during sorting is prohibited');}return $this->text;}
    function __destruct(){self::$drops[]=$this->text;}
}
$o=new ArrayObject(['z'=>new SortText('part9'),'a'=>new SortText('part2')]);SortText::$owner=$o;
same($o->natsort(),true);same(array_keys($o->getArrayCopy()),['a','z']);same(SortText::$calls>0,true);same(SortText::$drops,[]);
SortText::$owner=null;$o->exchangeArray([]);same(count(SortText::$drops),2);
$o=new ArrayObject(['b'=>2,'a'=>1]);$clone=null;$once=false;
$o->uasort(function($a,$b)use($o,&$clone,&$once){if(!$once){$once=true;$clone=clone $o;$clone['inside']=7;}return $a<=>$b;});
$clone['outside']=8;same($clone->getArrayCopy(),['b'=>2,'a'=>1,'inside'=>7,'outside'=>8]);same($o->getArrayCopy(),['a'=>1,'b'=>2]);
echo "objects:ok\n";
