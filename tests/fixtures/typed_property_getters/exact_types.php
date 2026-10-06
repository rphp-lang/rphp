<?php
interface GetterLeft {} interface GetterRight {}
class GetterBoth implements GetterLeft, GetterRight {}
class GetterValues {
    private $text = 'leaf'; private $items = [1, 2]; private $node;
    function __construct() { $this->node = new GetterBoth; }
    function text(): string { return $this->text; }
    function items(): array { return $this->items; }
    function node(): GetterLeft { return $this->node; }
    function either(): GetterLeft|GetterRight { return $this->node; }
    function both(): GetterLeft&GetterRight { return $this->node; }
}
class GetterShadow extends GetterValues { public $text = 'shadow'; public $items = [9]; }
$objects = [new GetterValues, new GetterShadow]; $sum = 0;
for ($i = 0; $i < 12; $i++) { $v = $objects[($i >> 2) & 1]; $sum += strlen($v->text()) + count($v->items()); $sum += ($v->node() === $v->either()) + ($v->both() === $v->node()); }
echo $sum, '|';
