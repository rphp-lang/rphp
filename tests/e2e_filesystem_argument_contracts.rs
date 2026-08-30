mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use common::run_php;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory(std::path::PathBuf);

impl TemporaryDirectory {
    fn new(label: &str) -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rphp-filesystem-contract-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn php_literal(&self) -> String {
        self.0
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn filesystem_functions_expose_php_85_signatures_and_named_arguments() {
    let fixture = TemporaryDirectory::new("calls");
    let root = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
foreach (['mkdir', 'rmdir', 'unlink', 'rename', 'copy', 'tempnam', 'glob'] as $name) {{
    $reflection = new ReflectionFunction($name);
    echo $name, '|', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), '|', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {{
        echo $parameter->getName(), ':',
            $parameter->hasType() ? $parameter->getType() : '-', ':';
        echo $parameter->isDefaultValueAvailable()
            ? var_export($parameter->getDefaultValue(), true)
            : '-';
        echo "\n";
    }}
}}
foreach ([
    'GLOB_ERR' => GLOB_ERR,
    'GLOB_MARK' => GLOB_MARK,
    'GLOB_NOCHECK' => GLOB_NOCHECK,
    'GLOB_NOSORT' => GLOB_NOSORT,
    'GLOB_BRACE' => GLOB_BRACE,
    'GLOB_NOESCAPE' => GLOB_NOESCAPE,
    'GLOB_ONLYDIR' => GLOB_ONLYDIR,
    'GLOB_AVAILABLE_FLAGS' => GLOB_AVAILABLE_FLAGS,
] as $name => $value) {{
    echo $name, '=', $value, "\n";
}}

$root = '{root}';
$source = $root . '/source.txt';
file_put_contents($source, 'payload');

$made = mkdir(directory: $root . '/mode', permissions: 0700, recursive: false, context: null);
echo 'mkdir=', $made ? 'true' : 'false', ':', fileperms($root . '/mode') & 0777, "\n";
$recursive = mkdir(directory: $root . '/nested/a', permissions: 0750, recursive: true, context: null);
echo 'mkdir-recursive=', $recursive ? 'true' : 'false', ':',
    fileperms($root . '/nested/a') & 0777, "\n";

$copied = copy(from: $source, to: $root . '/copy.txt', context: null);
echo 'copy=', $copied ? 'true' : 'false', ':', file_get_contents($root . '/copy.txt'), "\n";
$same = copy(from: $source, to: $source, context: null);
echo 'copy-same=', $same ? 'true' : 'false', ':', file_get_contents($source), "\n";
$previousUmask = umask(0022);
$permissionSource = $root . '/permission-source';
file_put_contents($permissionSource, 'permissions');
chmod($permissionSource, 0600);
copy($permissionSource, $root . '/permission-new');
file_put_contents($root . '/permission-existing', 'old');
chmod($root . '/permission-existing', 0666);
copy($permissionSource, $root . '/permission-existing');
echo 'copy-permissions=', fileperms($root . '/permission-new') & 0777, ':',
    fileperms($root . '/permission-existing') & 0777, "\n";
umask($previousUmask);
$renamed = rename(from: $root . '/copy.txt', to: $root . '/moved.txt', context: null);
echo 'rename=', $renamed ? 'true' : 'false', ':',
    file_exists($root . '/copy.txt') ? 'kept' : 'gone', ':',
    file_exists($root . '/moved.txt') ? 'moved' : 'missing', "\n";
$unlinked = unlink(filename: $root . '/moved.txt', context: null);
echo 'unlink=', $unlinked ? 'true' : 'false', ':',
    file_exists($root . '/moved.txt') ? 'kept' : 'gone', "\n";

$temporary = tempnam(directory: $root, prefix: 'pre');
echo 'tempnam=', is_string($temporary) && str_starts_with(basename($temporary), 'pre') ? 'true' : 'false',
    ':', is_file($temporary) ? 'true' : 'false', ':', fileperms($temporary) & 0777, "\n";
unlink($temporary);
$previousDirectory = getcwd();
chdir($root);
$relativeTemporary = tempnam('.', 'nested/relative');
chdir($previousDirectory);
echo 'tempnam-relative=', dirname($relativeTemporary) === $root ? 'true' : 'false', ':',
    str_starts_with(basename($relativeTemporary), 'relative') ? 'true' : 'false', "\n";
unlink($relativeTemporary);
$longTemporary = tempnam($root, str_repeat('p', 80));
echo 'tempnam-long=', substr(basename($longTemporary), 0, 63) === str_repeat('p', 63) ? 'true' : 'false',
    ':', strlen(basename($longTemporary)), "\n";
unlink($longTemporary);
$emptyDirectoryTemporary = tempnam('', 'empty');
echo 'tempnam-empty-directory=', dirname($emptyDirectoryTemporary) === sys_get_temp_dir() ? 'true' : 'false', "\n";
unlink($emptyDirectoryTemporary);
$empty = mkdir(directory: $root . '/empty');
$removed = rmdir(directory: $root . '/empty', context: null);
echo 'rmdir=', $empty && $removed ? 'true' : 'false', ':',
    file_exists($root . '/empty') ? 'kept' : 'gone', "\n";
"#
        )),
        concat!(
            "mkdir|1/4|bool\n",
            "directory:string:-\n",
            "permissions:int:511\n",
            "recursive:bool:false\n",
            "context:-:NULL\n",
            "rmdir|1/2|bool\n",
            "directory:string:-\n",
            "context:-:NULL\n",
            "unlink|1/2|bool\n",
            "filename:string:-\n",
            "context:-:NULL\n",
            "rename|2/3|bool\n",
            "from:string:-\n",
            "to:string:-\n",
            "context:-:NULL\n",
            "copy|2/3|bool\n",
            "from:string:-\n",
            "to:string:-\n",
            "context:-:NULL\n",
            "tempnam|2/2|string|false\n",
            "directory:string:-\n",
            "prefix:string:-\n",
            "glob|1/2|array|false\n",
            "pattern:string:-\n",
            "flags:int:0\n",
            "GLOB_ERR=4\n",
            "GLOB_MARK=8\n",
            "GLOB_NOCHECK=16\n",
            "GLOB_NOSORT=32\n",
            "GLOB_BRACE=128\n",
            "GLOB_NOESCAPE=4096\n",
            "GLOB_ONLYDIR=1073741824\n",
            "GLOB_AVAILABLE_FLAGS=1073746108\n",
            "mkdir=true:448\n",
            "mkdir-recursive=true:488\n",
            "copy=true:payload\n",
            "copy-same=false:payload\n",
            "copy-permissions=420:438\n",
            "rename=true:gone:moved\n",
            "unlink=true:gone\n",
            "tempnam=true:true:384\n",
            "tempnam-relative=true:true\n",
            "tempnam-long=true:82\n",
            "tempnam-empty-directory=true\n",
            "rmdir=true:gone\n",
        )
    );
}

#[test]
fn glob_honors_framework_visible_flags_and_path_components() {
    let fixture = TemporaryDirectory::new("glob");
    for file in ["a.txt", "b.txt", "7.txt", "literal*", ".hidden", "x", "x5"] {
        std::fs::write(fixture.0.join(file), b"fixture").unwrap();
    }
    for directory in ["a-dir", "b-dir"] {
        std::fs::create_dir(fixture.0.join(directory)).unwrap();
        std::fs::write(fixture.0.join(directory).join("nested.php"), b"fixture").unwrap();
    }
    let root = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
$root = '{root}';
$normalize = static fn(array $paths): array => array_map(
    static fn(string $path): string => str_replace($root, 'ROOT', $path),
    $paths,
);
foreach ([
    'plain' => glob($root . '/*.txt'),
    'brace' => glob($root . '/{{b,a}}.txt', 128),
    'single-brace' => glob($root . '/x{{5}}', GLOB_BRACE),
    'empty-brace' => glob($root . '/x{{}}', GLOB_BRACE),
    'onlydir' => glob($root . '/*-dir', 1073741824),
    'onlydir-nocheck' => glob($root . '/missing-*', GLOB_ONLYDIR | GLOB_NOCHECK),
    'mark' => glob($root . '/*', 8),
    'nocheck' => glob($root . '/missing-*', 16),
    'nested' => glob($root . '/*-dir/*.php'),
    'inner-double' => glob($root . '//a.txt'),
    'class' => glob($root . '/[ab].txt'),
    'posix-class' => glob($root . '/[[:digit:]].txt'),
    'escaped' => glob($root . '/literal\\*'),
    'dot' => glob($root . '/.*'),
] as $label => $paths) {{
    echo $label, '=', json_encode($normalize($paths), JSON_UNESCAPED_SLASHES), "\n";
}}
set_error_handler(static function (int $level, string $message): never {{
    throw new ErrorException($message, 0, $level);
}});
try {{ glob($root . '/*', 2); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
restore_error_handler();
"#
        )),
        concat!(
            "plain=[\"ROOT/7.txt\",\"ROOT/a.txt\",\"ROOT/b.txt\"]\n",
            "brace=[\"ROOT/b.txt\",\"ROOT/a.txt\"]\n",
            "single-brace=[\"ROOT/x5\"]\n",
            "empty-brace=[\"ROOT/x\"]\n",
            "onlydir=[\"ROOT/a-dir\",\"ROOT/b-dir\"]\n",
            "onlydir-nocheck=[]\n",
            "mark=[\"ROOT/7.txt\",\"ROOT/a-dir/\",\"ROOT/a.txt\",\"ROOT/b-dir/\",\"ROOT/b.txt\",\"ROOT/literal*\",\"ROOT/x\",\"ROOT/x5\"]\n",
            "nocheck=[\"ROOT/missing-*\"]\n",
            "nested=[\"ROOT/a-dir/nested.php\",\"ROOT/b-dir/nested.php\"]\n",
            "inner-double=[\"ROOT//a.txt\"]\n",
            "class=[\"ROOT/a.txt\",\"ROOT/b.txt\"]\n",
            "posix-class=[\"ROOT/7.txt\"]\n",
            "escaped=[\"ROOT/literal*\"]\n",
            "dot=[\"ROOT/.\",\"ROOT/..\",\"ROOT/.hidden\"]\n",
            "ErrorException:glob(): At least one of the passed flags is invalid or not supported on this platform\n",
        )
    );
}

#[cfg(unix)]
#[test]
fn glob_preserves_double_root_and_literal_dangling_symlinks() {
    use std::os::unix::fs::symlink;

    let fixture = TemporaryDirectory::new("glob-links");
    std::fs::write(fixture.0.join("a.txt"), b"fixture").unwrap();
    symlink("missing-target", fixture.0.join("dangling")).unwrap();
    let root = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
$root = '{root}';
$normalize = static fn(array $paths): array => array_map(
    static fn(string $path): string => str_replace($root, 'ROOT', $path),
    $paths,
);
echo 'double-root=', json_encode($normalize(glob('/' . $root . '/a.txt')), JSON_UNESCAPED_SLASHES), "\n";
echo 'dangling=', json_encode($normalize(glob($root . '/dangling', GLOB_MARK)), JSON_UNESCAPED_SLASHES), "\n";
"#
        )),
        concat!(
            "double-root=[\"/ROOT/a.txt\"]\n",
            "dangling=[\"ROOT/dangling\"]\n",
        )
    );
}

#[test]
fn filesystem_argument_errors_precede_side_effects() {
    let fixture = TemporaryDirectory::new("errors");
    let root = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php declare(strict_types=1);
$root = '{root}';
file_put_contents($root . '/source', 'payload');
$cases = [
    static fn() => mkdir(directory: 1),
    static fn() => mkdir(directory: $root . '/bad-mode', permissions: []),
    static fn() => unlink(filename: $root . '/missing', context: new stdClass),
    static fn() => copy(from: $root . '/missing', to: $root . '/copy', context: false),
    static fn() => glob(pattern: $root . '/*', flags: []),
    static fn() => tempnam(directory: "a\0b", prefix: 'x'),
    static fn() => copy(from: '', to: $root . '/copy'),
    static fn() => copy(from: $root . '/source', to: ''),
];
foreach ($cases as $case) {{
    try {{ $case(); }}
    catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
}}
echo file_exists($root . '/bad-mode') ? 'mutated' : 'clean', "\n";
"#
        )),
        concat!(
            "TypeError:mkdir(): Argument #1 ($directory) must be of type string, int given\n",
            "TypeError:mkdir(): Argument #2 ($permissions) must be of type int, array given\n",
            "TypeError:unlink(): Argument #2 ($context) must be of type resource or null, stdClass given\n",
            "TypeError:copy(): Argument #3 ($context) must be of type resource or null, false given\n",
            "TypeError:glob(): Argument #2 ($flags) must be of type int, array given\n",
            "ValueError:tempnam(): Argument #1 ($directory) must not contain any null bytes\n",
            "ValueError:Path must not be empty\n",
            "ValueError:Path must not be empty\n",
            "clean\n",
        )
    );
}

#[cfg(unix)]
#[test]
fn filesystem_failures_emit_php_diagnostics_and_tempnam_falls_back() {
    let fixture = TemporaryDirectory::new("failures");
    std::fs::create_dir(fixture.0.join("nonempty")).unwrap();
    std::fs::write(fixture.0.join("nonempty/file"), b"fixture").unwrap();
    let root = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
$root = '{root}';
set_error_handler(static function (int $level, string $message): bool {{
    echo 'diag=', $level, ':', $message, "\n";
    return true;
}});
echo 'mkdir-existing='; var_dump(mkdir($root));
echo 'mkdir-existing-recursive='; var_dump(mkdir($root, 0777, true));
echo 'rmdir-nonempty='; var_dump(rmdir($root . '/nonempty'));
echo 'rmdir-missing='; var_dump(rmdir($root . '/missing'));
echo 'unlink-missing='; var_dump(unlink($root . '/missing'));
echo 'rename-missing='; var_dump(rename($root . '/missing', $root . '/to'));
echo 'copy-missing='; var_dump(copy($root . '/missing', $root . '/to'));
echo 'copy-directory-source='; var_dump(copy($root . '/nonempty', $root . '/to'));
echo 'copy-directory-destination='; var_dump(copy($root . '/nonempty/file', $root));
echo 'tempnam-missing=';
$temporary = tempnam($root . '/missing', 'pre');
echo is_string($temporary) && is_file($temporary) ? "fallback\n" : "failed\n";
unlink($temporary);
restore_error_handler();
"#
        )),
        format!(
            concat!(
                "mkdir-existing=diag=2:mkdir(): File exists\n",
                "bool(false)\n",
                "mkdir-existing-recursive=diag=2:mkdir(): File exists\n",
                "bool(false)\n",
                "rmdir-nonempty=diag=2:rmdir({root}/nonempty): Directory not empty\n",
                "bool(false)\n",
                "rmdir-missing=diag=2:rmdir({root}/missing): No such file or directory\n",
                "bool(false)\n",
                "unlink-missing=diag=2:unlink({root}/missing): No such file or directory\n",
                "bool(false)\n",
                "rename-missing=diag=2:rename({root}/missing,{root}/to): No such file or directory\n",
                "bool(false)\n",
                "copy-missing=diag=2:copy({root}/missing): Failed to open stream: No such file or directory\n",
                "bool(false)\n",
                "copy-directory-source=diag=2:copy(): The first argument to copy() function cannot be a directory\n",
                "bool(false)\n",
                "copy-directory-destination=diag=2:copy(): The second argument to copy() function cannot be a directory\n",
                "bool(false)\n",
                "tempnam-missing=diag=8:tempnam(): file created in the system's temporary directory\n",
                "fallback\n",
            ),
            root = root,
        )
    );
}

#[cfg(feature = "stream-context")]
#[test]
fn filesystem_contexts_accept_contexts_and_defer_invalid_resources() {
    let fixture = TemporaryDirectory::new("context");
    let root = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
$root = '{root}';
$context = stream_context_create();
var_dump(mkdir(directory: $root . '/valid', context: $context));
var_dump(rmdir(directory: $root . '/valid', context: $context));
file_put_contents($root . '/ordinary-resource', 'fixture');
$stream = fopen($root . '/ordinary-resource', 'r');
try {{ mkdir(directory: $root . '/invalid', context: $stream); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
echo 'mkdir-state=', file_exists($root . '/invalid') ? 'made' : 'missing', "\n";
mkdir($root . '/remove');
try {{ rmdir(directory: $root . '/remove', context: $stream); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
echo 'rmdir-state=', file_exists($root . '/remove') ? 'kept' : 'removed', "\n";
file_put_contents($root . '/unlink', 'fixture');
try {{ unlink(filename: $root . '/unlink', context: $stream); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
echo 'unlink-state=', file_exists($root . '/unlink') ? 'kept' : 'removed', "\n";
file_put_contents($root . '/rename-from', 'fixture');
try {{ rename(from: $root . '/rename-from', to: $root . '/rename-to', context: $stream); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
echo 'rename-state=', file_exists($root . '/rename-to') ? 'renamed' : 'missing', "\n";
file_put_contents($root . '/copy-from', 'fixture');
try {{ copy(from: $root . '/copy-from', to: $root . '/copy-to', context: $stream); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
echo 'copy-state=', file_exists($root . '/copy-to') ? 'copied' : 'missing', "\n";
fclose($stream);
try {{ mkdir(directory: $root . '/closed', context: $stream); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
echo 'closed-state=', file_exists($root . '/closed') ? 'made' : 'missing', "\n";
"#
        )),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "TypeError:mkdir(): supplied resource is not a valid Stream-Context resource\n",
            "mkdir-state=made\n",
            "TypeError:rmdir(): supplied resource is not a valid Stream-Context resource\n",
            "rmdir-state=removed\n",
            "TypeError:unlink(): supplied resource is not a valid Stream-Context resource\n",
            "unlink-state=removed\n",
            "TypeError:rename(): supplied resource is not a valid Stream-Context resource\n",
            "rename-state=renamed\n",
            "TypeError:copy(): supplied resource is not a valid Stream-Context resource\n",
            "copy-state=copied\n",
            "TypeError:mkdir(): supplied resource is not a valid Stream-Context resource\n",
            "closed-state=made\n",
        )
    );
}
