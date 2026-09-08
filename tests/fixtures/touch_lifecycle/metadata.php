<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
$r = new ReflectionFunction('touch');
echo $r->getNumberOfRequiredParameters(), '/', $r->getNumberOfParameters(), ':', $r->getReturnType(), "\n";
foreach ($r->getParameters() as $p) {
    echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), ':', (int)$p->isPassedByReference(), "\n";
    if ($p->isOptional()) var_dump($p->getDefaultValue());
}
var_dump(touch(atime: 234, filename: 'named', mtime: 123));
echo filemtime('named'), ':', fileatime('named'), "\n";
try { touch(); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { touch('a', 1, 2, 3); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { touch(unknown: 'a'); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
