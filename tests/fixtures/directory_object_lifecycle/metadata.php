<?php
$f = new ReflectionFunction('dir');
echo $f->getNumberOfParameters(), '/', $f->getNumberOfRequiredParameters(), ':', $f->getReturnType(), "\n";
foreach ($f->getParameters() as $p) {
    echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), ':', (int)$p->isPassedByReference(), "\n";
}
$r = new ReflectionClass('Directory');
echo (int)$r->isFinal(), ':', (int)$r->isInternal(), ':', (int)$r->isInstantiable(), "\n";
foreach ($r->getProperties() as $p) {
    echo $p->getName(), ':', $p->getType(), ':', $p->getModifiers(), ':', (int)$p->hasDefaultValue(), "\n";
}
foreach ($r->getMethods() as $m) {
    echo $m->getName(), ':', $m->getNumberOfParameters(), ':', $m->getReturnType(), ':', (int)$m->hasTentativeReturnType(), "\n";
}
$rendered = (string)$r;
var_dump(str_contains($rendered, "Directory ] {\n\n"));
var_dump(str_contains((string)$r->getMethod('read'), "method read ] {\n\n"));
var_dump(str_contains($rendered, "    }\n\n    Method"));
