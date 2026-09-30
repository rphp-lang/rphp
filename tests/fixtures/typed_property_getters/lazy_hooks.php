<?php
class GetterLazy { public string $text = 'warm'; function read(): string { return $this->text; } }
class GetterHook { public string $text { get { echo 'hook|'; return 'backed'; } } function read(): string { return $this->text; } }
$normal = new GetterLazy; for ($i = 0; $i < 3; $i++) echo $normal->read(), '|';
$r = new ReflectionClass(GetterLazy::class); $ghost = $r->newLazyGhost(function($object) { echo 'init|'; $object->text = 'ghost'; });
for ($i = 0; $i < 3; $i++) echo $ghost->read(), '|';
$hook = new GetterHook; for ($i = 0; $i < 3; $i++) echo $hook->read(), '|';
