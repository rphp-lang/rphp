<?php
$root = tempnam(sys_get_temp_dir(), 'rphp-source-');
unlink($root); mkdir($root);
foreach (['caller', 'cwd', 'search'] as $dir) mkdir($root . '/' . $dir);
file_put_contents($root . '/caller/entry.php', <<<'PHP'
<?php
function selected($name, $search) {
    $stream = fopen($name, 'rb', $search);
    echo fread($stream, 32), "\n";
    fclose($stream);
}
$root = dirname(__DIR__);
$saved = getcwd();
chdir($root . '/cwd');
set_include_path($root . '/search' . PATH_SEPARATOR);
selected('value.txt', true);
unlink($root . '/search/value.txt');
selected('value.txt', true);
selected('./value.txt', true);
selected('value.txt', false);
selected($root . '/cwd/value.txt', true);
set_include_path('.' . PATH_SEPARATOR . $root . '/search');
selected('value.txt', true);
set_include_path($root . '/search' . PATH_SEPARATOR);
echo file_get_contents('value.txt', true), "\n";
echo file('value.txt', FILE_USE_INCLUDE_PATH)[0], "\n";
ob_start(); $count = readfile('value.txt', true); $bytes = ob_get_clean();
echo $bytes, ':', $count, "\n";
echo file_put_contents('value.txt', 'updated', FILE_USE_INCLUDE_PATH), ':', file_get_contents(__DIR__ . '/value.txt'), ':', file_get_contents($root . '/cwd/value.txt'), "\n";
unlink(__DIR__ . '/value.txt');
selected('value.txt', true);
chdir($saved);
PHP);
file_put_contents($root . '/caller/value.txt', 'source');
file_put_contents($root . '/cwd/value.txt', 'working');
file_put_contents($root . '/search/value.txt', 'search');
include $root . '/caller/entry.php';
unlink($root . '/caller/entry.php');
unlink($root . '/cwd/value.txt');
foreach (['caller', 'cwd', 'search'] as $dir) rmdir($root . '/' . $dir);
rmdir($root);
