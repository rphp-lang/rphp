mod common;

#[cfg(feature = "stream-registry")]
use common::run_php;
#[cfg(feature = "stream-registry")]
use std::process::Command;

#[test]
#[cfg(feature = "stream-registry")]
fn registry_functions_expose_php_85_signatures_and_alias_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['stream_wrapper_register', 'stream_register_wrapper', 'stream_wrapper_unregister', 'stream_wrapper_restore'] as $name) {
    $function = new ReflectionFunction($name);
    echo $function->getName(), ':', (string) $function->getReturnType(), ':';
    foreach ($function->getParameters() as $parameter) {
        echo '$', $parameter->getName(), '=', (string) $parameter->getType();
        if ($parameter->isDefaultValueAvailable()) {
            echo '[', var_export($parameter->getDefaultValue(), true), ']';
        }
        echo ';';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "stream_wrapper_register:bool:$protocol=string;$class=string;$flags=int[0];\n",
            "stream_register_wrapper:bool:$protocol=string;$class=string;$flags=int[0];\n",
            "stream_wrapper_unregister:bool:$protocol=string;\n",
            "stream_wrapper_restore:bool:$protocol=string;\n",
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn registry_rejects_invalid_changes_and_restores_builtin_wrappers() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diagnostic:$level:$message\n";
    return true;
});
class RegistryProbe { public $context; }
var_dump(stream_wrapper_register('CaseProbe', RegistryProbe::class, STREAM_IS_URL));
var_dump(in_array('CaseProbe', stream_get_wrappers(), true));
var_dump(stream_is_local('CaseProbe://value'));
var_dump(stream_wrapper_register('CaseProbe', RegistryProbe::class));
var_dump(stream_wrapper_unregister('caseprobe'));
var_dump(stream_wrapper_unregister('CaseProbe'));
var_dump(stream_wrapper_unregister('file'));
var_dump(stream_wrapper_register('file', RegistryProbe::class));
var_dump(stream_wrapper_restore('file'));
var_dump(in_array('file', stream_get_wrappers(), true));
var_dump(stream_wrapper_restore('CaseProbe'));
var_dump(stream_wrapper_register('', RegistryProbe::class));
var_dump(stream_wrapper_register('bad_name', RegistryProbe::class));
try {
    stream_wrapper_register('missing', 'MissingRegistryProbe');
} catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "diagnostic:2:stream_wrapper_register(): Protocol CaseProbe:// is already defined.\n",
            "bool(false)\n",
            "diagnostic:2:stream_wrapper_unregister(): Unable to unregister protocol caseprobe://\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "diagnostic:2:stream_wrapper_restore(): CaseProbe:// never existed, nothing to restore\n",
            "bool(false)\n",
            "bool(true)\n",
            "diagnostic:2:stream_wrapper_register(): Invalid protocol scheme specified. ",
            "Unable to register wrapper class RegistryProbe to bad_name://\n",
            "bool(false)\n",
            "TypeError:stream_wrapper_register(): Argument #2 ($class) must be a valid class name, ",
            "MissingRegistryProbe given\n",
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn registry_is_exact_case_and_live_handle_survives_self_unregister() {
    assert_eq!(
        run_php(
            r#"<?php
class ExactWrapper {
    public $context;
    private string $data = 'payload';
    private int $offset = 0;
    private function __construct() { echo 'constructed:'; }
    public function stream_open($path, $mode, $options, &$openedPath) {
        echo isset($this->context) ? 'context:' : 'missing:';
        $openedPath = 'Exact://canonical';
        echo stream_wrapper_unregister('Exact') ? 'unregistered:' : 'kept:';
        return true;
    }
    public function stream_read($count) {
        $chunk = substr($this->data, $this->offset, $count);
        $this->offset += strlen($chunk);
        return $chunk;
    }
    public function stream_eof() { return $this->offset >= strlen($this->data); }
    public function stream_close() { echo 'closed:'; }
}
echo stream_wrapper_register('Exact', ExactWrapper::class) ? 'registered:' : 'failed:';
echo in_array('Exact', stream_get_wrappers(), true) ? 'listed:' : 'missing:';
$stream = fopen('Exact://value', 'r');
echo stream_get_meta_data($stream)['uri'], ':';
echo fread($stream, 7), ':';
echo in_array('Exact', stream_get_wrappers(), true) ? 'stale:' : 'removed:';
echo fclose($stream) ? 'done' : 'bad';
"#,
        ),
        concat!(
            "registered:listed:constructed:context:unregistered:",
            "Exact://value:payload:removed:closed:done",
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn callback_exceptions_are_catchable_and_preserve_the_open_resource() {
    assert_eq!(
        run_php(
            r#"<?php
class ThrowingWrapper {
    public $context;
    public function stream_open() { return true; }
    public function stream_eof() { throw new RuntimeException('eof'); }
    public function stream_close() { echo 'close'; }
}
stream_wrapper_register('throwing', ThrowingWrapper::class);
$stream = fopen('throwing://value', 'r');
try { feof($stream); }
catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), ':'; }
echo is_resource($stream) ? 'live:' : 'lost:';
fclose($stream);
"#,
        ),
        "RuntimeException:eof:live:close"
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn directory_callbacks_rewind_metadata_and_close_once() {
    assert_eq!(
        run_php(
            r#"<?php
class DirectoryWrapper {
    public $context;
    private array $entries = ['one', 'two'];
    private int $offset = 0;
    public function dir_opendir($path, $options) { return true; }
    public function dir_readdir() { return $this->entries[$this->offset++] ?? false; }
    public function dir_rewinddir() { $this->offset = 0; return true; }
    public function dir_closedir() { echo 'close:'; return true; }
}
stream_wrapper_register('directory', DirectoryWrapper::class);
$directory = opendir('directory://root');
echo readdir($directory), ':', readdir($directory), ':';
rewinddir($directory);
echo readdir($directory), ':';
echo stream_get_meta_data($directory)['wrapper_data'] instanceof DirectoryWrapper ? 'object:' : 'bad:';
closedir($directory);
echo is_resource($directory) ? 'live' : 'closed';
"#,
        ),
        "one:two:one:object:close:closed"
    );
}

#[test]
#[cfg(all(feature = "stream-registry", feature = "include-path"))]
fn include_uses_opened_path_for_once_identity_and_reads_beyond_one_chunk() {
    assert_eq!(
        run_php(
            r#"<?php
class IncludeWrapper {
    public $context;
    private string $source = '';
    private int $offset = 0;
    public function stream_open($path, $mode, $options, &$openedPath) {
        $openedPath = 'include-probe://canonical.php';
        $this->source = '<?php echo "body:", __FILE__, ":"; return 31;';
        $this->source = substr($this->source, 0, 6) . str_repeat(' ', 9000) . substr($this->source, 6);
        return true;
    }
    public function stream_set_option() { return false; }
    public function stream_stat() { return array_fill(0, 13, 0); }
    public function stream_read($count) {
        $chunk = substr($this->source, $this->offset, $count);
        $this->offset += strlen($chunk);
        return $chunk;
    }
    public function stream_eof() { return $this->offset >= strlen($this->source); }
    public function stream_close() { echo 'close:'; }
}
stream_wrapper_register('include-probe', IncludeWrapper::class);
echo include_once 'include-probe://first.php'; echo ':';
echo include_once 'include-probe://second.php';
"#,
        ),
        concat!("close:body:include-probe://canonical.php:31:", "close:1",)
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn request_shutdown_closes_unreleased_wrapper_and_builtin_streams_keep_fast_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-r",
            r#"
class ShutdownWrapper {
    public $context;
    private string $path = '';
    public function stream_open($path) { $this->path = $path; return true; }
    public function stream_close() { echo 'close:', $this->path, ':'; }
}
$memory = fopen('php://memory', 'w+');
fwrite($memory, 'ok'); rewind($memory); echo fread($memory, 2), ':';
stream_wrapper_register('shutdown-probe', ShutdownWrapper::class);
$first = fopen('shutdown-probe://first', 'r');
$second = fopen('shutdown-probe://second', 'r');
echo get_resource_type($second), ':';
"#,
        ])
        .output()
        .expect("RPHP CLI must run the request-shutdown wrapper specimen");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "ok:stream:",
            "close:shutdown-probe://second:",
            "close:shutdown-probe://first:",
        )
    );
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}
