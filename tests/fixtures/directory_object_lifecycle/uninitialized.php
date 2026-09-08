<?php
$r = new ReflectionClass('Directory');
$d = $r->newInstanceWithoutConstructor();
foreach (['path', 'handle'] as $field) {
    $p = $r->getProperty($field);
    var_dump($p->isInitialized($d));
    try { echo $d->$field; } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
foreach (['read', 'rewind', 'close'] as $method) {
    try { $d->$method(); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
try { $r->newInstance(); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { $r->newInstanceArgs([]); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
class_alias('Directory', 'DirectoryAliasForSpec');
try { new DirectoryAliasForSpec; } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
