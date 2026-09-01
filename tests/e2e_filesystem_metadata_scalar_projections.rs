mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use common::run_php;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct MetadataFixture {
    root: std::path::PathBuf,
    file: std::path::PathBuf,
    directory: std::path::PathBuf,
    link: std::path::PathBuf,
}

impl MetadataFixture {
    fn new() -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rphp-filesystem-metadata-{}-{sequence}",
            std::process::id()
        ));
        let file = root.join("file.txt");
        let directory = root.join("directory");
        let link = root.join("link.txt");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&file, b"metadata").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
            symlink("file.txt", &link).unwrap();
        }
        Self {
            root,
            file,
            directory,
            link,
        }
    }

    fn php_literal(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for MetadataFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn metadata_scalar_globals_expose_php_85_signatures_and_extension_identity() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['fileowner', 'filegroup', 'fileinode', 'fileperms', 'filetype', 'linkinfo', 'fstat'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '|', $function->getExtensionName(), '|',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(),
        '|', (string) $function->getReturnType(), '|';
    foreach ($function->getParameters() as $parameter) {
        echo '$', $parameter->getName(), ':', (string) $parameter->getType(), ';';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "fileowner|standard|1/1|int|false|$filename:string;\n",
            "filegroup|standard|1/1|int|false|$filename:string;\n",
            "fileinode|standard|1/1|int|false|$filename:string;\n",
            "fileperms|standard|1/1|int|false|$filename:string;\n",
            "filetype|standard|1/1|string|false|$filename:string;\n",
            "linkinfo|standard|1/1|int|false|$path:string;\n",
            "fstat|standard|1/1|array|false|$stream:;\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn local_scalar_projections_share_stat_and_lstat_identity() {
    let fixture = MetadataFixture::new();
    let file = MetadataFixture::php_literal(&fixture.file);
    let directory = MetadataFixture::php_literal(&fixture.directory);
    let link = MetadataFixture::php_literal(&fixture.link);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$stat = stat('{file}');
$linkStat = stat('{link}');
$linkLstat = lstat('{link}');
echo (int) (fileowner('{file}') === $stat['uid']), ':',
    (int) (filegroup('{file}') === $stat['gid']), ':',
    (int) (fileinode('{file}') === $stat['ino']), ':',
    (int) (fileperms('{file}') === $stat['mode']), '|';
echo (int) (fileinode('{link}') === $linkStat['ino']), ':',
    (int) ($linkStat['ino'] === $stat['ino']), ':',
    (int) ($linkLstat['ino'] !== $stat['ino']), '|';
echo filetype('{file}'), ':', filetype('{directory}'), ':', filetype('{link}'), '|';
echo (int) (linkinfo('{file}') === $stat['dev']), ':',
    (int) (linkinfo('{link}') === $linkLstat['dev']), "\n";
"#
        )),
        "1:1:1:1|1:1:1|file:dir:link|1:1\n"
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn user_wrapper_scalar_projection_uses_php_flags_cache_and_file_kinds() {
    assert_eq!(
        run_php(
            r#"<?php
class MetadataProjectionWrapper {
    public $context;
    public static array $calls = [];
    public static int $mode = 0100644;
    public function url_stat($path, $flags) {
        self::$calls[] = substr($path, strlen('metadata://')) . ':' . $flags;
        return ['dev' => 77, 'ino' => 78, 'mode' => self::$mode, 'nlink' => 1,
            'uid' => 79, 'gid' => 80, 'rdev' => 0, 'size' => 81,
            'atime' => 82, 'mtime' => 83, 'ctime' => 84,
            'blksize' => 4096, 'blocks' => 8];
    }
}
stream_wrapper_register('metadata', MetadataProjectionWrapper::class);
echo fileowner('metadata://item'), ':', filegroup('metadata://item'), ':',
    fileinode('metadata://item'), ':', fileperms('metadata://item'), ':',
    filetype('metadata://item'), '|', implode(',', MetadataProjectionWrapper::$calls), "\n";
echo 'kinds=';
foreach ([0010000, 0020000, 0040000, 0060000, 0100000, 0120000, 0140000] as $mode) {
    clearstatcache();
    MetadataProjectionWrapper::$mode = $mode;
    echo filetype('metadata://kind'), ',';
}
echo "\n";
clearstatcache();
MetadataProjectionWrapper::$mode = 0;
set_error_handler(static function (int $level, string $message): bool {
    echo $level, ':', $message, '|';
    return true;
});
var_dump(filetype('metadata://unknown'));
"#,
        ),
        concat!(
            "79:80:78:33188:file|item:4,item:5\n",
            "kinds=fifo,char,dir,block,file,link,socket,\n",
            "8:filetype(): Unknown file type (0)|string(7) \"unknown\"\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn scalar_failures_preserve_php_warning_silence_and_nul_boundaries() {
    let fixture = MetadataFixture::new();
    let missing = MetadataFixture::php_literal(&fixture.root.join("missing"));
    assert_eq!(
        run_php(&format!(
            r#"<?php
$missing = '{missing}';
set_error_handler(static function (int $level, string $message) use ($missing): bool {{
    echo $level, ':', str_replace($missing, '<missing>', $message), "\n";
    return true;
}});
foreach (['fileowner', 'filegroup', 'fileinode', 'fileperms'] as $name) {{
    echo $name, '='; var_dump($name($missing));
}}
echo 'filetype='; var_dump(filetype($missing));
echo 'empty=';
foreach (['fileowner', 'filegroup', 'fileinode', 'fileperms', 'filetype'] as $name) {{
    echo get_debug_type($name(''));
}}
echo "\n";
echo 'nul='; var_dump(fileowner("bad\0path"));
try {{ linkinfo("bad\0path"); }}
catch (Throwable $error) {{ echo $error::class, ':', $error->getMessage(), "\n"; }}
echo 'linkinfo='; var_dump(linkinfo($missing));
"#
        )),
        concat!(
            "fileowner=2:fileowner(): stat failed for <missing>\nbool(false)\n",
            "filegroup=2:filegroup(): stat failed for <missing>\nbool(false)\n",
            "fileinode=2:fileinode(): stat failed for <missing>\nbool(false)\n",
            "fileperms=2:fileperms(): stat failed for <missing>\nbool(false)\n",
            "filetype=2:filetype(): Lstat failed for <missing>\nbool(false)\n",
            "empty=boolboolboolboolbool\n",
            "nul=2:fileowner(): Filename contains null byte\nbool(false)\n",
            "ValueError:linkinfo(): Argument #1 ($path) must not contain any null bytes\n",
            "linkinfo=2:linkinfo(): No such file or directory\nint(-1)\n",
        )
    );
}

#[test]
fn strict_scalar_contracts_reject_arrays_without_mutating_references() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach (['fileowner', 'filegroup', 'fileinode', 'fileperms', 'filetype', 'linkinfo'] as $name) {
    try { $name([]); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$path = 'missing';
$alias =& $path;
@fileowner($alias);
echo $path, ':', $alias, "\n";
"#,
        ),
        concat!(
            "TypeError:fileowner(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:filegroup(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:fileinode(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:fileperms(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:filetype(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:linkinfo(): Argument #1 ($path) must be of type string, array given\n",
            "missing:missing\n",
        )
    );
}

#[test]
#[cfg(all(unix, feature = "stream-registry"))]
fn linkinfo_remains_local_and_does_not_dispatch_user_url_stat() {
    assert_eq!(
        run_php(
            r#"<?php
class LinkInfoProjectionWrapper {
    public $context;
    public static int $calls = 0;
    public function url_stat($path, $flags) {
        self::$calls++;
        return ['dev' => 77, 'mode' => 0100644];
    }
}
stream_wrapper_register('linkmeta', LinkInfoProjectionWrapper::class);
set_error_handler(static function (int $level, string $message): bool {
    echo $level, ':', $message, '|';
    return true;
});
var_dump(linkinfo('linkmeta://item'));
echo 'calls=', LinkInfoProjectionWrapper::$calls, "\n";
"#,
        ),
        concat!(
            "2:linkinfo(): No such file or directory|int(-1)\n",
            "calls=0\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn fstat_plain_file_matches_path_stat_and_rejects_closed_streams() {
    let fixture = MetadataFixture::new();
    let file = MetadataFixture::php_literal(&fixture.file);
    let directory = MetadataFixture::php_literal(&fixture.directory);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$pathStat = stat('{file}');
$stream = fopen('{file}', 'rb');
$streamStat = fstat($stream);
$same = true;
foreach ($pathStat as $key => $value) {{
    $same = $same && $streamStat[$key] === $value;
}}
echo count($streamStat), ':', (int) $same, ':', $streamStat['size'], "\n";
fclose($stream);
try {{ fstat($stream); }}
catch (Throwable $error) {{ echo $error::class, ':', $error->getMessage(), "\n"; }}
$directory = opendir('{directory}');
var_dump(fstat($directory));
closedir($directory);
"#
        )),
        concat!(
            "26:1:8\n",
            "TypeError:fstat(): Argument #1 ($stream) must be an open stream resource\n",
            "bool(false)\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn fstat_standard_streams_publish_complete_alias_consistent_arrays() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['stdin' => STDIN, 'stdout' => STDOUT, 'stderr' => STDERR] as $name => $stream) {
    $stat = fstat($stream);
    echo $name, '=', get_debug_type($stat), ':', count($stat), ':',
        (int) ($stat[0] === $stat['dev']), ':',
        (int) ($stat[2] === $stat['mode']), "\n";
}
"#,
        ),
        concat!(
            "stdin=array:26:1:1\n",
            "stdout=array:26:1:1\n",
            "stderr=array:26:1:1\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn fstat_memory_and_temp_streams_publish_php_virtual_or_spilled_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['php://memory', 'php://temp'] as $uri) {
    $stream = fopen($uri, 'w+');
    fwrite($stream, 'abc');
    $stat = fstat($stream);
    echo $uri, '=', count($stat), ':', $stat['size'], ':',
        (int) (($stat['mode'] & 0170000) === 0100000), ':',
        (int) (($stat['mode'] & 0777) === 0666), "\n";
    fclose($stream);
    $stream = fopen($uri, 'r');
    $stat = fstat($stream);
    echo $uri, '-readonly=', (int) ($stat['mode'] === 0100444), "\n";
    fclose($stream);
}
$stream = fopen('php://temp/maxmemory:1', 'w+');
fwrite($stream, 'abc');
$stat = fstat($stream);
echo 'spill=', count($stat), ':', $stat['size'], ':',
    (int) (($stat['mode'] & 0170000) === 0100000), "\n";
fclose($stream);
"#,
        ),
        concat!(
            "php://memory=26:3:1:1\n",
            "php://memory-readonly=1\n",
            "php://temp=26:3:1:1\n",
            "php://temp-readonly=1\n",
            "spill=26:3:1\n",
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn fstat_user_wrapper_dispatches_stream_stat_and_normalizes_named_fields() {
    assert_eq!(
        run_php(
            r#"<?php
class StreamStatProjectionWrapper {
    public $context;
    public static int $calls = 0;
    public function stream_open($path, $mode, $options, &$openedPath) { return true; }
    public function stream_close() {}
    public function stream_stat() {
        self::$calls++;
        return ['dev' => 11, 'ino' => 22, 'mode' => 0100644, 'nlink' => 1,
            'uid' => 33, 'gid' => 44, 'rdev' => 0, 'size' => 55,
            'atime' => 66, 'mtime' => 77, 'ctime' => 88,
            'blksize' => 4096, 'blocks' => 8];
    }
}
stream_wrapper_register('streamstat', StreamStatProjectionWrapper::class);
$stream = fopen('streamstat://item', 'r');
$stat = fstat($stream);
echo count($stat), ':', $stat[0], ':', $stat['ino'], ':', $stat['size'], ':',
    StreamStatProjectionWrapper::$calls, "\n";
fclose($stream);
stream_wrapper_unregister('streamstat');
class MissingStreamStatWrapper {
    public $context;
    public function stream_open($path, $mode, $options, &$openedPath) { return true; }
    public function stream_close() {}
}
stream_wrapper_register('nostat', MissingStreamStatWrapper::class);
$stream = fopen('nostat://item', 'r');
set_error_handler(static function (int $level, string $message): bool {
    echo $level, ':', $message, '|';
    return true;
});
var_dump(fstat($stream));
fclose($stream);
stream_wrapper_unregister('nostat');
class DirectoryStatProjectionWrapper {
    public $context;
    public static int $calls = 0;
    public function dir_opendir($path, $options) { return true; }
    public function dir_readdir() { return false; }
    public function dir_closedir() { return true; }
    public function stream_stat() { self::$calls++; return array_fill(0, 13, 1); }
}
stream_wrapper_register('dirstat', DirectoryStatProjectionWrapper::class);
$directory = opendir('dirstat://item');
var_dump(fstat($directory));
echo 'directory-calls=', DirectoryStatProjectionWrapper::$calls, "\n";
closedir($directory);
"#,
        ),
        concat!(
            "26:11:22:55:1\n",
            "2:fstat(): MissingStreamStatWrapper::stream_stat is not implemented!|bool(false)\n",
            "bool(false)\n",
            "directory-calls=0\n",
        )
    );
}

#[test]
#[cfg(unix)]
fn metadata_globals_support_named_dynamic_first_class_and_callback_dispatch() {
    let fixture = MetadataFixture::new();
    let file = MetadataFixture::php_literal(&fixture.file);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$path = '{file}';
$dynamic = 'fileowner';
$firstClass = filetype(...);
$stream = fopen($path, 'rb');
echo (int) ($dynamic($path) === stat($path)['uid']), ':',
    $firstClass(filename: $path), ':',
    (int) (call_user_func('filegroup', $path) === stat($path)['gid']), ':',
    (int) (linkinfo(path: $path) === lstat($path)['dev']), ':',
    fstat(stream: $stream)['size'], "\n";
fclose($stream);
"#
        )),
        "1:file:1:1:8\n"
    );
}
