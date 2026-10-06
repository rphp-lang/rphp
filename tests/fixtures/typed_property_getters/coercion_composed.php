<?php
class GetterConvertible { function __toString(): string { echo 'convert|'; return 'text'; } }
class GetterCoercions { public $value = 2;
    function truth(): bool { return $this->value; }
    function number(): float { return $this->value; }
    function text(): string { return $this->value; }
    function either(): float|string { return $this->value; }
}
class GetterAccumulator { public int $total = 0; function add(int $value): void { $this->total += $value; } }
$g = new GetterCoercions; $a = new GetterAccumulator;
for ($i = 0; $i < 5; $i++) { $g->truth(); $a->add($g->truth()); echo get_debug_type($g->number()), ':', get_debug_type($g->text()), ':', get_debug_type($g->either()), '|'; }
echo $a->total, '|'; $g->value = new GetterConvertible; echo $g->text(), '|';
$g->value = []; try { $g->text(); } catch (TypeError $e) { echo 'text-error|'; }
