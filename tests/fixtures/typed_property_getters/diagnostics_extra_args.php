<?php
class GetterDiagnostic { public $text = 'ok'; #[Deprecated('use another reader')] function read(): string { return $this->text; } }
class GetterExtra { public $text = 'extra'; function read(): string { return $this->text; } }
set_error_handler(function($level, $message) { echo 'diagnostic|'; return true; });
$d = new GetterDiagnostic; for ($i = 0; $i < 3; $i++) echo $d->read(), '|'; restore_error_handler();
$x = new GetterExtra; for ($i = 0; $i < 3; $i++) echo $x->read(print 'arg|'), '|';
