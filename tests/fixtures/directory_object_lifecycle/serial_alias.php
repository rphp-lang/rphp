<?php
class_alias('Directory', 'DirectoryWireAliasForSpec');
foreach (['O', 'C'] as $tag) {
    $name = 'DirectoryWireAliasForSpec';
    $wire = $tag . ':' . strlen($name) . ':"' . $name . '":0:{}';
    try { unserialize($wire); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
spl_autoload_register(function($name) {
    echo "load:", $name, "\n";
    class_alias('Directory', $name);
});
$name = 'LoadedDirectoryAliasForSpec';
$wire = 'O:' . strlen($name) . ':"' . $name . '":0:{}';
try { unserialize($wire); }
catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
echo get_class(unserialize('O:9:"Directory":0:{}', ['allowed_classes' => false])), "\n";
