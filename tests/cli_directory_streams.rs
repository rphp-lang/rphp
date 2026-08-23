mod common;

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TemporaryDirectory(std::path::PathBuf);

impl TemporaryDirectory {
    fn unique() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rphp-cli-directory-streams-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct WorkingDirectoryGuard(std::path::PathBuf);

impl WorkingDirectoryGuard {
    fn current() -> Self {
        Self(std::env::current_dir().unwrap())
    }
}

impl Drop for WorkingDirectoryGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

#[test]
fn request_shutdown_restores_the_process_working_directory() {
    let root = TemporaryDirectory::unique();
    std::fs::create_dir(root.0.join("sub")).unwrap();
    let guard = WorkingDirectoryGuard::current();
    let source = format!(
        r#"<?php chdir('{}'); echo basename(getcwd()), "\n";"#,
        root.0.join("sub").to_string_lossy()
    );

    assert_eq!(common::run_php(&source), "sub\n");
    assert_eq!(std::env::current_dir().unwrap(), guard.0);
}

#[test]
fn directory_stream_lifecycle_scanning_diagnostics_and_cwd_match_php_85() {
    let root = TemporaryDirectory::unique();
    std::fs::create_dir(root.0.join("sub")).unwrap();
    for name in ["10", "2", "A", "a"] {
        std::fs::write(root.0.join(name), b"x").unwrap();
    }
    let source_path = root.0.join("main.php");
    let root_literal = root.0.to_string_lossy();
    let source = format!(
        r#"<?php
$root = '{root_literal}';
foreach (['chdir', 'opendir', 'readdir', 'rewinddir', 'closedir', 'scandir'] as $name) {{
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {{
        echo $parameter->getName(), ',', $parameter->isOptional() ? 'optional' : 'required', ';';
    }}
    echo "\n";
}}
echo 'constants:', SCANDIR_SORT_ASCENDING, ',', SCANDIR_SORT_DESCENDING, ',',
    SCANDIR_SORT_NONE, "\n";

echo 'scan-asc:', json_encode(scandir($root)), "\n";
echo 'scan-desc:', json_encode(scandir($root, SCANDIR_SORT_DESCENDING)), "\n";
$none = scandir($root, SCANDIR_SORT_NONE);
sort($none);
echo 'scan-none-set:', json_encode($none), "\n";

$handle = opendir($root);
echo 'resource:', get_resource_type($handle), ':', is_resource($handle) ? 'yes' : 'no', "\n";
$entries = [];
while (($entry = readdir($handle)) !== false) {{
    $entries[] = $entry;
}}
sort($entries);
echo 'read-set:', json_encode($entries), "\n";
var_dump(readdir($handle));
rewinddir($handle);
$first = readdir($handle);
readdir($handle);
rewinddir($handle);
echo 'rewind:', readdir($handle) === $first ? 'same' : 'different', "\n";
var_dump(closedir($handle));
echo 'closed:', get_resource_type($handle), ':', is_resource($handle) ? 'yes' : 'no', "\n";
try {{ readdir($handle); }} catch (Throwable $error) {{
    echo get_class($error), ':', $error->getMessage(), "\n";
}}

set_error_handler(function ($level, $message) {{
    echo 'diag:', $level, ':', $message, "\n";
    return true;
}});
$implicit = opendir($root . '/sub');
echo 'implicit:', readdir(), ',', readdir(), "\n";
var_dump(closedir());
try {{ readdir(); }} catch (Throwable $error) {{
    echo get_class($error), ':', $error->getMessage(), "\n";
}}
restore_error_handler();

$file = fopen(__FILE__, 'r');
try {{ closedir($file); }} catch (Throwable $error) {{
    echo get_class($error), ':', $error->getMessage(), "\n";
}}
fclose($file);

set_error_handler(function ($level, $message) use ($root) {{
    echo 'warning:', $level, ':', str_replace($root, 'ROOT', $message), "\n";
    mkdir($root . '/missing');
    return true;
}});
var_dump(opendir($root . '/missing'));
echo 'created:', is_dir($root . '/missing') ? 'yes' : 'no', "\n";
restore_error_handler();
rmdir($root . '/missing');

set_error_handler(function ($level, $message) use ($root) {{
    throw new RuntimeException(str_replace($root, 'ROOT', $message));
}});
try {{ scandir($root . '/missing'); }} catch (Throwable $error) {{
    echo get_class($error), ':', $error->getMessage(), "\n";
}}
restore_error_handler();

try {{ scandir(''); }} catch (Throwable $error) {{
    echo get_class($error), ':', $error->getMessage(), "\n";
}}

$before = getcwd();
var_dump(chdir($root . '/sub'));
echo 'cwd:', basename(getcwd()), "\n";
$relative = opendir('..');
echo 'relative:', get_resource_type($relative), "\n";
closedir($relative);
var_dump(chdir($before));
"#
    );
    std::fs::write(&source_path, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(&source_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "chdir:1/1:directory,required;\n",
            "opendir:1/2:directory,required;context,optional;\n",
            "readdir:0/1:dir_handle,optional;\n",
            "rewinddir:0/1:dir_handle,optional;\n",
            "closedir:0/1:dir_handle,optional;\n",
            "scandir:1/3:directory,required;sorting_order,optional;context,optional;\n",
            "constants:0,1,2\n",
            "scan-asc:[\".\",\"..\",\"10\",\"2\",\"A\",\"a\",\"main.php\",\"sub\"]\n",
            "scan-desc:[\"sub\",\"main.php\",\"a\",\"A\",\"2\",\"10\",\"..\",\".\"]\n",
            "scan-none-set:[\".\",\"..\",\"2\",\"10\",\"A\",\"a\",\"main.php\",\"sub\"]\n",
            "resource:stream:yes\n",
            "read-set:[\".\",\"..\",\"2\",\"10\",\"A\",\"a\",\"main.php\",\"sub\"]\n",
            "bool(false)\n",
            "rewind:same\n",
            "NULL\n",
            "closed:Unknown:no\n",
            "TypeError:readdir(): Argument #1 ($dir_handle) must be an open stream resource\n",
            "implicit:diag:8192:readdir(): Passing null is deprecated, instead the last opened directory stream should be provided\n",
            ".,diag:8192:readdir(): Passing null is deprecated, instead the last opened directory stream should be provided\n",
            "..\n",
            "diag:8192:closedir(): Passing null is deprecated, instead the last opened directory stream should be provided\n",
            "NULL\n",
            "diag:8192:readdir(): Passing null is deprecated, instead the last opened directory stream should be provided\n",
            "TypeError:No resource supplied\n",
            "TypeError:closedir(): Argument #1 ($dir_handle) must be a valid Directory resource\n",
            "warning:2:opendir(ROOT/missing): Failed to open directory: No such file or directory\n",
            "bool(false)\n",
            "created:yes\n",
            "RuntimeException:scandir(ROOT/missing): Failed to open directory: No such file or directory\n",
            "ValueError:scandir(): Argument #1 ($directory) must not be empty\n",
            "bool(true)\n",
            "cwd:sub\n",
            "relative:stream\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn directory_stream_strict_types_and_context_validation_match_php_85() {
    let source = r#"<?php
declare(strict_types=1);

function show_error(string $label, Closure $operation): void {
    try {
        $operation();
    } catch (Throwable $error) {
        echo $label, ':', get_class($error), ':', $error->getMessage(), "\n";
    }
}

$root = sys_get_temp_dir();
$file = fopen('php://memory', 'r');
show_error('opendir-directory', fn() => opendir(false));
show_error('opendir-context', fn() => opendir($root, $file));
show_error('scandir-order', fn() => scandir($root, false));
show_error('scandir-context', fn() => scandir($root, 0, $file));
show_error('readdir-handle', fn() => readdir(false));
show_error('rewinddir-handle', fn() => rewinddir(false));
show_error('closedir-handle', fn() => closedir(false));

if (function_exists('stream_context_create')) {
    $context = stream_context_create();
    $handle = opendir($root, $context);
    echo 'valid-context:', is_resource($handle) ? 'yes' : 'no', ',';
    closedir($handle);
    echo is_array(scandir($root, 0, $context)) ? 'yes' : 'no', "\n";
} else {
    echo "valid-context:unavailable\n";
}
fclose($file);
"#;
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg("-r")
        .arg(source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let context_result = if cfg!(feature = "stream-context") {
        "valid-context:yes,yes\n"
    } else {
        "valid-context:unavailable\n"
    };
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            concat!(
                "opendir-directory:TypeError:opendir(): Argument #1 ($directory) must be of type string, false given\n",
                "opendir-context:TypeError:opendir(): supplied resource is not a valid Stream-Context resource\n",
                "scandir-order:TypeError:scandir(): Argument #2 ($sorting_order) must be of type int, false given\n",
                "scandir-context:TypeError:scandir(): supplied resource is not a valid Stream-Context resource\n",
                "readdir-handle:TypeError:readdir(): Argument #1 ($dir_handle) must be of type resource or null, false given\n",
                "rewinddir-handle:TypeError:rewinddir(): Argument #1 ($dir_handle) must be of type resource or null, false given\n",
                "closedir-handle:TypeError:closedir(): Argument #1 ($dir_handle) must be of type resource or null, false given\n",
                "{}"
            ),
            context_result
        )
    );
}
