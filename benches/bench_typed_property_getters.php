<?php
interface TypedGetterLeaf {}
class TypedGetterValue implements TypedGetterLeaf { public int $number = 7; }
class TypedGetterBox {
    private string $name = 'leaf';
    private array $items = [1, 2];
    private TypedGetterLeaf $leaf;
    function __construct() { $this->leaf = new TypedGetterValue; }
    function name(): string { return $this->name; }
    function items(): array { return $this->items; }
    function leaf(): TypedGetterLeaf { return $this->leaf; }
    function nullable(): ?string { return $this->name; }
}
class TypedGetterShadow extends TypedGetterBox { public string $name = 'shadow'; }
$boxes = [new TypedGetterBox, new TypedGetterShadow];
$checksum = 0;
$start = microtime(true);
for ($i = 0; $i < 500_000; $i++) {
    $box = $boxes[$i & 1];
    $checksum += strlen($box->name()) + count($box->items()) + $box->leaf()->number + strlen($box->nullable());
}
echo $checksum, '|', microtime(true) - $start, "\n";
