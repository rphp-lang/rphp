mod common;

use common::run_php;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/phar/hello.phar"
);

/// The fixture is built by `tests/fixtures/phar/build.php` with reference PHP
/// (SHA-256 signed, uncompressed, four members plus an empty directory). The
/// expected output below is what PHP 8.5 prints for the same script, with the
/// fixture path folded to `FIX`/`PHAR` and the environment-dependent
/// `include_path` value removed.
#[test]
#[cfg(feature = "file-contents")]
fn phar_wrapper_reads_stats_lists_includes_and_maps_archives_like_php() {
    let source = r##"<?php
$p = '__FIXTURE__';
$u = "phar://$p";
$n = fn($v) => str_replace(["phar://$p", $p], ['PHAR', 'FIX'], is_string($v) ? $v : json_encode($v));
echo (int) extension_loaded('phar'), (int) extension_loaded('PHAR'), (int) in_array('Phar', get_loaded_extensions(), true), (int) class_exists('Phar'), (int) is_subclass_of('PharException', 'Exception'), "\n";
echo (int) Phar::isValidPharFilename($p), (int) Phar::isValidPharFilename('/nonexistent-dir/y.phar'), (int) Phar::isValidPharFilename(dirname($p) . '/new.phar'), (int) Phar::isValidPharFilename(dirname($p) . '/new.phar', false), (int) Phar::isValidPharFilename(dirname($p) . '/new.tar', false), (int) Phar::isValidPharFilename(dirname($p) . '/.phar'), "\n";
echo json_encode([Phar::running(), Phar::running(false)]), "\n";
echo (int) Phar::loadPhar($p, 'demo.phar'), "\n";
echo json_encode([is_file("$u/lib/greet.php"), is_dir("$u/lib"), is_dir("$u/lib/"), is_dir("$u/empty"), is_dir($u), is_file($u), file_exists("$u/nope"), is_file("$u/lib"), realpath("$u/data.txt"), filesize("$u/data.txt"), is_readable("$u/data.txt"), is_writable("$u/data.txt"), is_file("$u/lib/../data.txt"), file_exists("$u/./lib")]), "\n";
$s = stat("$u/data.txt"); echo decoct($s['mode']), ',', $s['size'], ',', (int) ($s['mtime'] === filemtime("$u/data.txt")), ',', decoct(stat("$u/lib")['mode']), ',', decoct(stat($u)['mode']), "\n";
echo json_encode([scandir($u), scandir("$u/lib"), scandir("$u/empty"), glob("$u/*"), @scandir("$u/nope")]), "\n";
$d = opendir("$u/lib"); $names = []; while (($e = readdir($d)) !== false) { $names[] = $e; } closedir($d); echo implode(',', $names), "\n";
echo json_encode([file_get_contents("$u/data.txt"), file_get_contents("$u/data.txt", false, null, 6, 4), file("$u/lib/greet.php", FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES), md5_file("$u/data.txt") === md5("plain dataé\n"), hash_file('sha256', "$u/data.txt") === hash('sha256', "plain dataé\n"), hash('sha512', 'abc')]), "\n";
$h = fopen("$u/data.txt", 'r'); fseek($h, 6); echo json_encode([fread($h, 4), ftell($h), feof($h), fstat($h)['size'], fgets($h), feof($h)]), "\n"; fclose($h);
$f = new SplFileObject("$u/lib/greet.php"); echo json_encode([$f->fgets(), $f->getFilename()]), "\n";
echo json_encode([@fopen("$u/nope", 'r'), @fopen("$u/data.txt", 'w'), @file_put_contents("$u/data.txt", 'x'), @file_get_contents("$u/nope"), @is_file('phar://nonexistent.phar/x'), @opendir("$u/nope"), @readfile("$u/nope"), @is_dir('phar://demo.phar'), is_dir('phar://demo.phar/'), is_file('phar://demo.phar/data.txt')]), "\n";
require "$u/lib/greet.php";
$info = require 'phar://demo.phar/lib/info.php';
echo Hello\greet('phar'), '|', $n($info['dir']), '|', $n($info['file']), '|', $n($info['running']), '|', $n($info['plain']), "\n";
echo json_encode([include_once "$u/lib/greet.php", (include "$u/lib/info.php")['running'] === $u]), '|', implode(',', array_map($n, array_values(array_filter(get_included_files(), fn($f) => str_starts_with($f, 'phar://'))))), "\n";
set_error_handler(function ($no, $str) use ($n) { echo "E:", preg_replace("~\\(include_path='[^']*'\\)~", '(include_path)', $n($str)), "\n"; return true; });
var_dump(fopen("$u/nope", 'r'), file_get_contents("$u/nope"), opendir("$u/nope"), scandir("$u/nope"), md5_file("$u/nope"), file("$u/nope"), fopen("$u/data.txt", 'w'), file_get_contents('phar://nonexistent.phar/x'));
try { include "$u/nope.php"; } catch (Throwable $e) { echo get_class($e), "\n"; }
restore_error_handler();
$plain = sys_get_temp_dir() . '/rphp-plain-' . getmypid() . '.php';
file_put_contents($plain, "<?php echo 1;");
try { Phar::loadPhar($plain); } catch (PharException $e) { echo get_class($e), ': ', str_replace($plain, 'PLAIN', $e->getMessage()), "\n"; }
unlink($plain);
try { Phar::loadPhar('/nonexistent/x.phar'); } catch (PharException $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { Phar::loadPhar($p, 'other.phar'); echo "alias-ok\n"; } catch (PharException $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
$broken = sys_get_temp_dir() . '/rphp-broken-' . getmypid() . '.phar';
$bytes = file_get_contents($p); $bytes[strlen($bytes) - 40] = chr(ord($bytes[strlen($bytes) - 40]) ^ 1); file_put_contents($broken, $bytes);
try { Phar::loadPhar($broken); echo "broken-loaded\n"; } catch (PharException $e) { echo get_class($e), ': ', str_replace($broken, 'BROKEN', $e->getMessage()), "\n"; }
echo json_encode([@is_file("phar://$broken/data.txt"), @file_get_contents("phar://$broken/data.txt")]), "\n";
unlink($broken);
echo json_encode([Phar::PHAR, Phar::TAR, Phar::ZIP, Phar::NONE, Phar::GZ, Phar::BZ2, Phar::COMPRESSED, Phar::PHP, Phar::PHPS, Phar::MD5, Phar::SHA1, Phar::SHA256, Phar::SHA512, Phar::OPENSSL, Phar::OPENSSL_SHA256, Phar::OPENSSL_SHA512, Phar::canWrite(), (new ReflectionMethod('Phar', 'mapPhar'))->isStatic(), (new ReflectionMethod('Phar', 'running'))->getNumberOfParameters(), (new ReflectionMethod('Phar', 'mapPhar'))->getExtensionName()]), "\n";
"##.replace("__FIXTURE__", FIXTURE);
    assert_eq!(
        run_php(&source),
        r##"11111
101010
["",""]
1
[true,true,true,true,true,false,false,false,false,13,true,false,true,true]
100444,13,1,40555,40555
[["data.txt","empty","lib"],["greet.php","info.php"],[],[],false]
greet.php,info.php
["plain data\u00e9\n","data",["<?php","namespace Hello;","function greet(string $n): string { return \"hello $n\"; }"],true,true,"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"]
["data",10,false,13,"\u00e9\n",true]
["<?php\n","greet.php"]
[false,false,false,false,false,false,false,false,true,true]
hello phar|PHAR/lib|PHAR/lib/info.php|PHAR|FIX
[true,true]|PHAR/lib/greet.php,PHAR/lib/info.php
E:fopen(PHAR/nope): Failed to open stream: phar error: "nope" is not a file in phar "FIX"
E:file_get_contents(PHAR/nope): Failed to open stream: phar error: "nope" is not a file in phar "FIX"
E:opendir(PHAR/nope): Failed to open directory: operation failed
E:scandir(PHAR/nope): Failed to open directory: operation failed
E:scandir(): (errno 2): No such file or directory
E:md5_file(PHAR/nope): Failed to open stream: phar error: "nope" is not a file in phar "FIX"
E:file(PHAR/nope): Failed to open stream: phar error: "nope" is not a file in phar "FIX"
E:fopen(PHAR/data.txt): Failed to open stream: phar error: write operations disabled by the php.ini setting phar.readonly
E:file_get_contents(phar://nonexistent.phar/x): Failed to open stream: phar error: invalid url or non-existent phar "phar://nonexistent.phar/x"
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
E:include(PHAR/nope.php): Failed to open stream: phar error: "nope.php" is not a file in phar "FIX"
E:include(): Failed opening 'PHAR/nope.php' for inclusion (include_path)
PharException: internal corruption of phar "PLAIN" (truncated entry)
PharException: unable to open phar for reading "/nonexistent/x.phar"
alias-ok
PharException: phar "BROKEN" SHA256 signature could not be verified: broken signature
[false,false]
[1,2,3,0,4096,8192,61440,0,1,1,2,3,4,16,17,18,false,true,1,"Phar"]
"##
    );
}

#[test]
fn phar_archive_root_and_alias_urls_follow_php_directory_rules() {
    let source = r##"<?php
$p = '__FIXTURE__';
Phar::loadPhar($p, 'root.phar');
var_dump(is_dir("phar://$p"), is_file("phar://$p"), is_dir("phar://$p/."), file_exists("phar://$p/./lib"), is_dir("phar://root.phar"), is_dir("phar://root.phar/"), is_file("phar://root.phar/data.txt"));
var_dump(count(scandir("phar://root.phar/")), (new ReflectionClass('Phar'))->getConstant('SHA512'), Phar::getSupportedSignatures());
"##
    .replace("__FIXTURE__", FIXTURE);
    assert_eq!(
        run_php(&source),
        "bool(true)\nbool(false)\nbool(true)\nbool(true)\nbool(false)\nbool(true)\nbool(true)\nint(3)\nint(4)\narray(4) {\n  [0]=>\n  string(3) \"MD5\"\n  [1]=>\n  string(5) \"SHA-1\"\n  [2]=>\n  string(7) \"SHA-256\"\n  [3]=>\n  string(7) \"SHA-512\"\n}\n"
    );
}
