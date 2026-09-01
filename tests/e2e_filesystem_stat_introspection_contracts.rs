mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use common::run_php;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct FilesystemFixture {
    root: std::path::PathBuf,
    file: std::path::PathBuf,
    script: std::path::PathBuf,
    directory: std::path::PathBuf,
    link: std::path::PathBuf,
    broken: std::path::PathBuf,
}

impl FilesystemFixture {
    fn new() -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rphp-filesystem-stat-{}-{sequence}",
            std::process::id()
        ));
        let file = root.join("file.txt");
        let script = root.join("script.sh");
        let directory = root.join("directory");
        let link = root.join("link.txt");
        let broken = root.join("broken.txt");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&file, b"abcdef").unwrap();
        std::fs::write(&script, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o750)).unwrap();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            symlink("file.txt", &link).unwrap();
            symlink("missing.txt", &broken).unwrap();
        }
        Self {
            root,
            file,
            script,
            directory,
            link,
            broken,
        }
    }

    fn php_literal(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for FilesystemFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn filesystem_stat_globals_expose_php_85_signatures_defaults_and_alias_names() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'clearstatcache', 'disk_free_space', 'diskfreespace', 'disk_total_space',
    'file_exists', 'fileatime', 'filectime', 'filemtime', 'filesize', 'is_dir',
    'is_executable', 'is_file', 'is_readable', 'is_writable', 'is_writeable',
    'lstat', 'realpath', 'stat', 'sys_get_temp_dir',
] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '|', $function->getName(), '|', $function->getExtensionName(), '|',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(),
        '|', (string) $function->getReturnType(), '|';
    foreach ($function->getParameters() as $parameter) {
        echo '$', $parameter->getName(), ':', (string) $parameter->getType();
        if ($parameter->isDefaultValueAvailable()) {
            echo '=', var_export($parameter->getDefaultValue(), true);
        }
        echo ';';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "clearstatcache|clearstatcache|standard|0/2|void|$clear_realpath_cache:bool=false;$filename:string='';\n",
            "disk_free_space|disk_free_space|standard|1/1|float|false|$directory:string;\n",
            "diskfreespace|diskfreespace|standard|1/1|float|false|$directory:string;\n",
            "disk_total_space|disk_total_space|standard|1/1|float|false|$directory:string;\n",
            "file_exists|file_exists|standard|1/1|bool|$filename:string;\n",
            "fileatime|fileatime|standard|1/1|int|false|$filename:string;\n",
            "filectime|filectime|standard|1/1|int|false|$filename:string;\n",
            "filemtime|filemtime|standard|1/1|int|false|$filename:string;\n",
            "filesize|filesize|standard|1/1|int|false|$filename:string;\n",
            "is_dir|is_dir|standard|1/1|bool|$filename:string;\n",
            "is_executable|is_executable|standard|1/1|bool|$filename:string;\n",
            "is_file|is_file|standard|1/1|bool|$filename:string;\n",
            "is_readable|is_readable|standard|1/1|bool|$filename:string;\n",
            "is_writable|is_writable|standard|1/1|bool|$filename:string;\n",
            "is_writeable|is_writeable|standard|1/1|bool|$filename:string;\n",
            "lstat|lstat|standard|1/1|array|false|$filename:string;\n",
            "realpath|realpath|standard|1/1|string|false|$path:string;\n",
            "stat|stat|standard|1/1|array|false|$filename:string;\n",
            "sys_get_temp_dir|sys_get_temp_dir|standard|0/0|string|\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn local_files_dirs_links_times_and_permissions_share_php_stat_identity() {
    let fixture = FilesystemFixture::new();
    let file = FilesystemFixture::php_literal(&fixture.file);
    let script = FilesystemFixture::php_literal(&fixture.script);
    let directory = FilesystemFixture::php_literal(&fixture.directory);
    let link = FilesystemFixture::php_literal(&fixture.link);
    let broken = FilesystemFixture::php_literal(&fixture.broken);
    let missing = FilesystemFixture::php_literal(&fixture.root.join("missing.txt"));
    assert_eq!(
        run_php(&format!(
            r#"<?php
$paths = ['{file}', '{script}', '{directory}', '{link}', '{broken}', '{missing}'];
foreach (['file_exists', 'is_file', 'is_dir', 'is_readable', 'is_writable', 'is_writeable', 'is_executable'] as $name) {{
    echo $name, '=';
    foreach ($paths as $path) echo (int) $name($path);
    echo "\n";
}}
$stat = stat('{file}');
$linkStat = stat('{link}');
$linkLstat = lstat('{link}');
$brokenLstat = lstat('{broken}');
echo 'shape=', count($stat), ':', implode(',', array_keys($stat)), "\n";
echo 'values=', filesize('{file}'), ':', $stat['size'], ':', $stat[7], ':',
    (int) ($stat['atime'] === fileatime('{file}')), ':',
    (int) ($stat['mtime'] === filemtime('{file}')), ':',
    (int) ($stat['ctime'] === filectime('{file}')), ':',
    (int) ($stat['ino'] === $linkStat['ino']), ':',
    (int) ($linkLstat['ino'] !== $stat['ino']), ':',
    (int) (($linkLstat['mode'] & 0170000) === 0120000), ':',
    (int) (($brokenLstat['mode'] & 0170000) === 0120000), "\n";
echo 'realpath=', (int) (realpath('{link}') === '{file}'), ':',
    get_debug_type(realpath('{missing}')), "\n";
echo 'file-uri=', (int) file_exists('file://localhost{file}'), ':',
    filesize('file://localhost{file}'), ':', (int) is_file('FILE://{file}'), "\n";
file_put_contents('{file}', 'abcdefghij');
echo 'cache=', filesize('{file}'), ':';
clearstatcache();
echo filesize('{file}'), '|';
echo (int) file_exists('{missing}'), ':';
file_put_contents('{missing}', 'x');
echo (int) file_exists('{missing}'), "\n";
"#
        )),
        concat!(
            "file_exists=111100\n",
            "is_file=110100\n",
            "is_dir=001000\n",
            "is_readable=111100\n",
            "is_writable=111100\n",
            "is_writeable=111100\n",
            "is_executable=011000\n",
            "shape=26:0,1,2,3,4,5,6,7,8,9,10,11,12,dev,ino,mode,nlink,uid,gid,rdev,size,atime,mtime,ctime,blksize,blocks\n",
            "values=6:6:6:1:1:1:1:1:1:1\n",
            "realpath=1:bool\n",
            "file-uri=1:6:1\n",
            "cache=10:10|0:1\n",
        )
    );
}

#[test]
#[cfg(all(
    feature = "stream-truncate",
    feature = "file-contents",
    feature = "file-lines"
))]
fn stream_and_direct_read_attempts_invalidate_only_the_php_stat_cache_boundaries() {
    let fixture = FilesystemFixture::new();
    let file = FilesystemFixture::php_literal(&fixture.file);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$handle = fopen('{file}', 'r+');
echo 'truncate-read=', filesize('{file}'), ':';
ftruncate($handle, 2);
echo filesize('{file}'), ':';
$memory = fopen('php://memory', 'w+');
fwrite($memory, 'memory-only');
echo filesize('{file}'), ':';
fclose($memory);
fread($handle, 1);
echo filesize('{file}'), "\n";
ftruncate($handle, 1);
echo 'flush-write=', filesize('{file}'), ':';
fflush($handle);
echo filesize('{file}'), ':';
fseek($handle, 0, SEEK_END);
fwrite($handle, 'xyz');
echo filesize('{file}'), "\n";
fclose($handle);
$handle = fopen('{file}', 'r+');
echo 'direct-read=', filesize('{file}'), ':';
ftruncate($handle, 1);
fclose($handle);
echo filesize('{file}'), ':';
file_get_contents('{file}');
echo filesize('{file}'), ':';
$handle = fopen('{file}', 'r+');
echo filesize('{file}'), ':';
ftruncate($handle, 0);
file('{file}');
echo filesize('{file}'), "\n";
fclose($handle);
"#
        )),
        concat!(
            "truncate-read=6:6:6:2\n",
            "flush-write=2:1:4\n",
            "direct-read=4:4:1:1:0\n",
        )
    );
}

#[test]
fn stat_failures_dispatch_through_php_error_handlers_and_remain_catchable() {
    let fixture = FilesystemFixture::new();
    let missing = FilesystemFixture::php_literal(&fixture.root.join("missing.txt"));
    assert_eq!(
        run_php(&format!(
            r#"<?php
$missing = '{missing}';
set_error_handler(static function (int $level, string $message) use ($missing): bool {{
    echo $level, ':', str_replace($missing, '<missing>', $message), "\n";
    return true;
}});
foreach (['filesize', 'fileatime', 'filemtime', 'filectime', 'stat', 'lstat'] as $name) {{
    echo $name, '='; var_dump($name($missing));
}}
echo 'quiet=', (int) file_exists($missing), (int) is_file($missing), (int) is_dir($missing), "\n";
set_error_handler(static function (): never {{ throw new RuntimeException('handler-stop'); }});
try {{ filesize($missing); }} catch (Throwable $error) {{
    echo $error::class, ':', $error->getMessage(), "\n";
}}
"#
        )),
        concat!(
            "filesize=2:filesize(): stat failed for <missing>\nbool(false)\n",
            "fileatime=2:fileatime(): stat failed for <missing>\nbool(false)\n",
            "filemtime=2:filemtime(): stat failed for <missing>\nbool(false)\n",
            "filectime=2:filectime(): stat failed for <missing>\nbool(false)\n",
            "stat=2:stat(): stat failed for <missing>\nbool(false)\n",
            "lstat=2:lstat(): Lstat failed for <missing>\nbool(false)\n",
            "quiet=000\nRuntimeException:handler-stop\n",
        )
    );
}

#[test]
fn disk_space_functions_and_historical_alias_return_php_floats() {
    let fixture = FilesystemFixture::new();
    let root = FilesystemFixture::php_literal(&fixture.root);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$free = disk_free_space(directory: '{root}');
$alias = diskfreespace('{root}');
$total = disk_total_space('{root}');
echo get_debug_type($free), ':', (int) ($free > 0), ':',
    get_debug_type($alias), ':', (int) ($alias > 0), ':',
    get_debug_type($total), ':', (int) ($total >= $free), "\n";
"#
        )),
        "float:1:float:1:float:1\n"
    );
}

#[test]
fn disk_space_failures_use_invoked_alias_names_and_php_error_dispatch() {
    let fixture = FilesystemFixture::new();
    let missing = FilesystemFixture::php_literal(&fixture.root.join("missing"));
    assert_eq!(
        run_php(&format!(
            r#"<?php
$missing = '{missing}';
set_error_handler(static function (int $level, string $message): bool {{
    echo $level, ':', $message, "\n";
    return true;
}});
foreach (['disk_free_space', 'diskfreespace', 'disk_total_space'] as $name) {{
    var_dump($name($missing));
}}
"#
        )),
        concat!(
            "2:disk_free_space(): No such file or directory\nbool(false)\n",
            "2:diskfreespace(): No such file or directory\nbool(false)\n",
            "2:disk_total_space(): No such file or directory\nbool(false)\n",
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn user_wrapper_stat_projection_preserves_values_flags_and_cache_categories() {
    assert_eq!(
        run_php(
            r#"<?php
class StatProjectionWrapper {
    public $context;
    public static array $calls = [];
    public function url_stat($path, $flags) {
        self::$calls[] = substr($path, strlen('statprojection://')) . ':' . $flags;
        $mode = str_contains($path, 'directory') ? 0040755 : 0100644;
        return ['dev' => 11, 'ino' => 22, 'mode' => $mode, 'nlink' => 1,
            'uid' => 33, 'gid' => 44, 'rdev' => 0, 'size' => 123,
            'atime' => 101, 'mtime' => 202, 'ctime' => 303,
            'blksize' => 4096, 'blocks' => 8];
    }
}
stream_wrapper_register('statprojection', StatProjectionWrapper::class);
echo (int) file_exists('statprojection://file'), ':',
    (int) is_file('statprojection://file'), ':',
    (int) is_dir('statprojection://directory'), '|';
echo filesize('statprojection://file'), ':', fileatime('statprojection://file'), ':',
    filemtime('statprojection://file'), ':', filectime('statprojection://file'), '|';
$stat = stat('statprojection://file');
$lstat = lstat('statprojection://file');
echo count($stat), ':', $stat[7], ':', $stat['size'], ':', $stat['mtime'], '|',
    count($lstat), ':', $lstat['size'], ':', $lstat['mtime'], '|';
$stat['size'] = 999;
echo filesize('statprojection://file'), '|', implode(',', StatProjectionWrapper::$calls), "\n";
"#,
        ),
        "1:1:1|123:101:202:303|26:123:123:202|26:123:202|123|file:6,directory:6,file:4,file:5\n"
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn clearstatcache_invalidates_user_wrapper_results_without_repeating_hot_queries() {
    assert_eq!(
        run_php(
            r#"<?php
class StatCacheWrapper {
    public $context;
    public static int $calls = 0;
    public static int $size = 10;
    public function url_stat($path, $flags) {
        self::$calls++;
        if (str_ends_with($path, '/absent')) return false;
        $mode = str_ends_with($path, '/link') && ($flags & 1) ? 0120777 : 0100644;
        return ['mode' => $mode, 'size' => self::$size];
    }
}
stream_wrapper_register('statcache', StatCacheWrapper::class);
echo (int) file_exists('statcache://file'), ':', filesize('statcache://file'), ':';
StatCacheWrapper::$size = 20;
echo filesize('statcache://file'), ':', StatCacheWrapper::$calls, '|';
stat('statcache://other');
echo filesize('statcache://file'), ':', StatCacheWrapper::$calls, '|';
echo (int) file_exists('statcache://absent'), (int) file_exists('statcache://absent'),
    ':', StatCacheWrapper::$calls, '|';
StatCacheWrapper::$size = 30;
clearstatcache(false);
echo filesize('statcache://file'), ':', StatCacheWrapper::$calls, '|';
$dynamic = 'clearstatcache';
$firstClass = clearstatcache(...);
echo get_debug_type($dynamic(clear_realpath_cache: false)), ':',
    get_debug_type($firstClass(false, 'statcache://file')), '|';
clearstatcache();
StatCacheWrapper::$size = 40;
echo lstat('statcache://file')['size'], ':';
StatCacheWrapper::$size = 50;
echo filesize('statcache://file'), ':', StatCacheWrapper::$calls, '|';
clearstatcache();
StatCacheWrapper::$size = 60;
echo lstat('statcache://link')['size'], ':';
StatCacheWrapper::$size = 70;
echo filesize('statcache://link'), ':', StatCacheWrapper::$calls, '|';
stream_wrapper_unregister('statcache');
echo filesize('statcache://link'), ':', (int) is_file('statcache://link'), ':',
    (int) file_exists('statcache://link'), ':', (int) is_readable('statcache://link'), "\n";
"#,
        ),
        "1:10:10:1|20:3|00:5|30:6|null:null|40:40:7|60:70:9|70:1:0:0\n"
    );
}

#[test]
fn stat_functions_support_named_dynamic_first_class_and_callback_dispatch() {
    let fixture = FilesystemFixture::new();
    let file = FilesystemFixture::php_literal(&fixture.file);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$path = '{file}';
$dynamic = 'filesize';
$firstClass = stat(...);
echo filesize(filename: $path), ':', $dynamic($path), ':',
    $firstClass(filename: $path)['size'], ':',
    call_user_func('filemtime', $path), ':',
    call_user_func_array('file_exists', ['filename' => $path]) ? 'yes' : 'no', ':',
    get_debug_type(sys_get_temp_dir()), "\n";
"#
        )),
        format!(
            "6:6:6:{}:yes:string\n",
            std::fs::metadata(&fixture.file)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        )
    );
}

#[test]
fn string_contracts_reject_strict_arrays_and_nul_paths_without_mutating_references() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach (['file_exists', 'filesize', 'disk_total_space', 'realpath'] as $name) {
    try { $name([]); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
try { file_exists("bad\0path"); }
catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
echo 'nul=', (int) file_exists("bad\0path"), ':', (int) is_file("bad\0path"), "\n";
set_error_handler(static function (int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
var_dump(filesize("bad\0path"));
$path = 'missing';
$alias =& $path;
echo (int) file_exists($alias), ':', $path, ':', $alias, "\n";
"#,
        ),
        concat!(
            "TypeError:file_exists(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:filesize(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:disk_total_space(): Argument #1 ($directory) must be of type string, array given\n",
            "TypeError:realpath(): Argument #1 ($path) must be of type string, array given\n",
            "nul=0:0\n",
            "2:filesize(): Filename contains null byte\nbool(false)\n",
            "0:missing:missing\n",
        )
    );
}

#[test]
fn temp_directory_and_realpath_are_typed_quiet_and_case_insensitive() {
    let fixture = FilesystemFixture::new();
    let root = FilesystemFixture::php_literal(&fixture.root);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$temp = sys_get_temp_dir();
echo get_debug_type($temp), ':', (int) ($temp !== ''), ':', (int) IS_DIR($temp), '|';
echo (int) (realpath('{root}/.') === '{root}'), ':',
    get_debug_type(realpath('{root}/missing')), ':',
    (int) function_exists('DISKFREESPACE'), ':',
    (int) (realpath('') === getcwd()), ':',
    get_debug_type(disk_free_space('')), "\n";
"#
        )),
        "string:1:1|1:bool:1:1:bool\n"
    );
}
