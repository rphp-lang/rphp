<?php
class GetterUninitialized { public string $text; function read(): string { return $this->text; } }
class GetterMagic { public $text = 'plain'; function read(): string { return $this->text; } function __get($n) { echo 'magic|'; return 'generated'; } }
$x = new GetterUninitialized; $x->text = 'warm'; for ($i = 0; $i < 3; $i++) echo $x->read(), '|';
$empty = new GetterUninitialized; try { $empty->read(); } catch (Error $e) { echo 'uninitialized|'; }
$m = new GetterMagic; for ($i = 0; $i < 3; $i++) echo $m->read(), '|'; unset($m->text); echo $m->read(), '|';
