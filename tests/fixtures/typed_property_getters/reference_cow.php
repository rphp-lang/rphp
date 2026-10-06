<?php
class GetterAliases { public $items = [1]; public $text = 'cat'; public $cell = 3;
    function items(): array { return $this->items; }
    function text(): string { return $this->text; }
    function &cell(): int { return $this->cell; }
}
$x = new GetterAliases; $items =& $x->items; $text =& $x->text;
for ($i = 0; $i < 5; $i++) { $copy = $x->items(); $copy[] = $i; echo count($items), ':', count($copy), '|'; }
$items[] = 2; echo count($x->items()), '|'; $text = 'dog'; echo $x->text(), '|';
$ref =& $x->cell(); $ref = 7; echo $x->cell, '|';
$items = 42; try { $x->items(); } catch (TypeError $e) { echo 'array-error|'; }
