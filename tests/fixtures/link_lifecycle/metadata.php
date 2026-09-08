<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
foreach (['link', 'symlink', 'readlink', 'is_link'] as $name) {
    $r = new ReflectionFunction($name);
    echo $name, ':', $r->getNumberOfRequiredParameters(), '/', $r->getNumberOfParameters(), ':', $r->getReturnType(), "\n";
    foreach ($r->getParameters() as $p) echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), ':', (int)$p->isPassedByReference(), "\n";
}
file_put_contents('source', 'ok');
var_dump(link(link: 'hard', target: 'source'), symlink(link: 'soft', target: 'source'), readlink(path: 'soft'));
foreach (['link', 'symlink', 'readlink'] as $name) {
    try { $name(); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
    try { $name('a', 'b', 'c'); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
    try { $name(unknown: 'a'); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
}
