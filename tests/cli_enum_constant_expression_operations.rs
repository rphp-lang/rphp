use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "log_errors=0",
            "-d",
            "html_errors=0",
            "-d",
            "fatal_error_backtraces=0",
            "-d",
            "zend.exception_ignore_args=1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn enum_runtime_variable_uses_case_line_and_canonical_compile_fatal() {
    let cases = [
        (
            "direct",
            "<?php\necho \"never\\n\";\nenum Signal: int {\n    case Bad = 1 + $value;\n}\n",
            4,
        ),
        (
            "nested",
            "<?php\nnamespace Oracle;\nenum Signal: int {\n    case Bad = [0, 1 + $value][1];\n}\n",
            4,
        ),
        (
            "multiline",
            "<?php\nenum Signal: int {\n    case Bad = 1\n        + $value;\n}\n",
            3,
        ),
    ];

    for (label, source, line) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(
            stderr,
            format!(
                "Fatal error: Constant expression contains invalid operations in Standard input code on line {line}\n"
            ),
            "{label}"
        );
    }
}

#[test]
fn enum_constant_operation_keeps_shape_and_global_error_priority() {
    let cases = [
        (
            "missing backing",
            "<?php\nenum Signal: int {\n    case Missing;\n    case Bad = 1 + $value;\n}\n",
            "Fatal error: Case Missing of backed enum Signal must have a value in Standard input code on line 3\n",
        ),
        (
            "property",
            "<?php\nenum Signal: int {\n    public int $property;\n    case Bad = 1 + $value;\n}\n",
            "Fatal error: Enum Signal cannot include properties in Standard input code on line 3\n",
        ),
        (
            "magic",
            "<?php\nenum Signal: int {\n    case Bad = 1 + $value;\n    public function __construct() {}\n}\n",
            "Fatal error: Constant expression contains invalid operations in Standard input code on line 3\n",
        ),
        (
            "interface",
            "<?php\nenum Signal: int implements UnitEnum {\n    case Bad = 1 + $value;\n}\n",
            "Fatal error: Constant expression contains invalid operations in Standard input code on line 3\n",
        ),
        (
            "method body",
            "<?php\nenum Signal: int {\n    case Bad = 1 + $value;\n    public function run(): void {\n        break;\n    }\n}\n",
            "Fatal error: Constant expression contains invalid operations in Standard input code on line 3\n",
        ),
        (
            "later syntax",
            "<?php\nenum Signal: int {\n    case Bad = 1 + $value;\n}\nisset(, $value);\n",
            "Parse error: syntax error, unexpected token \",\" in Standard input code on line 5\n",
        ),
    ];

    for (label, source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(stderr, expected, "{label}");
    }
}

#[test]
fn enum_constant_operation_is_validated_in_elided_and_post_return_declarations() {
    let cases = [
        (
            "elided",
            "<?php\nif (false) {\n    enum Signal: int {\n        case Bad = 1 + $value;\n    }\n}\necho \"after\\n\";\n",
            4,
        ),
        (
            "post return",
            "<?php\nfunction loadSignal(): void {\n    return;\n    enum Signal: int {\n        case Bad = 1 + $value;\n    }\n}\necho \"after\\n\";\n",
            5,
        ),
    ];

    for (label, source, line) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(
            stderr,
            format!(
                "Fatal error: Constant expression contains invalid operations in Standard input code on line {line}\n"
            ),
            "{label}"
        );
    }
}

#[test]
fn enum_object_offset_is_lazy_catchable_repeated_and_skips_array_access() {
    let source = r#"<?php
enum Signal implements ArrayAccess {
    case Ready;
    public function offsetGet($key): mixed { echo "offsetGet\n"; return 42; }
    public function offsetExists($key): bool { echo "offsetExists\n"; return true; }
    public function offsetSet($key, $value): void { echo "offsetSet\n"; }
    public function offsetUnset($key): void { echo "offsetUnset\n"; }
}
class Holder {
    public const BAD = Signal::Ready[0];
}
echo "declared\n";
foreach ([1, 2] as $attempt) {
    try {
        var_dump(Holder::BAD);
    } catch (Throwable $error) {
        echo $attempt, ':', get_class($error), ':', $error->getMessage(), '@', $error->getLine(), "\n";
    }
}
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "declared\n1:Error:Cannot use [] on objects in constant expression@10\n2:Error:Cannot use [] on objects in constant expression@10\nafter\n"
    );
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn enum_object_offset_precedes_index_and_respects_short_circuit_controls() {
    let source = r#"<?php
enum Signal implements ArrayAccess {
    case Ready;
    public function offsetGet($key): mixed { echo "offsetGet\n"; return 42; }
    public function offsetExists($key): bool { return true; }
    public function offsetSet($key, $value): void {}
    public function offsetUnset($key): void {}
}
class Holder {
    public const BAD = Signal::Ready[MissingKey::VALUE];
    public const AND_VALUE = false && Signal::Ready[0];
    public const TERNARY_VALUE = true ? 7 : Signal::Ready[0];
    public const FIRST = [10, 20][0];
    public const SECOND = ['key' => 30]['key'];
}
try {
    var_dump(Holder::BAD);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '@', $error->getLine(), "\n";
}
var_dump(Holder::AND_VALUE, Holder::TERNARY_VALUE, Holder::FIRST, Holder::SECOND);
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "Error:Cannot use [] on objects in constant expression@10\nbool(false)\nint(7)\nint(10)\nint(30)\n"
    );
    assert!(stderr.is_empty(), "{stderr:?}");
}

struct IncludeFixture {
    directory: std::path::PathBuf,
    invalid_enum: std::path::PathBuf,
    object_offset: std::path::PathBuf,
}

impl IncludeFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_enum_constant_expression_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let invalid_enum = directory.join("invalid-enum.php");
        std::fs::write(
            &invalid_enum,
            "<?php\nnamespace Included;\nenum Signal: int {\n    case Bad = 1 + $value;\n}\n",
        )
        .unwrap();
        let object_offset = directory.join("object-offset.php");
        std::fs::write(
            &object_offset,
            r#"<?php
namespace Deferred;
enum Signal implements \ArrayAccess {
    case Ready;
    public function offsetGet($key): mixed { echo "offsetGet\n"; return 42; }
    public function offsetExists($key): bool { return true; }
    public function offsetSet($key, $value): void {}
    public function offsetUnset($key): void {}
}
class Holder {
    public const BAD = Signal::Ready[0];
}
"#,
        )
        .unwrap();
        Self {
            directory,
            invalid_enum,
            object_offset,
        }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn include_compile_fatal_and_deferred_error_preserve_source_state_and_origin() {
    let fixture = IncludeFixture::new();
    let invalid_driver = format!(
        "<?php\nregister_shutdown_function(function () {{ echo \"shutdown\\n\"; }});\necho \"before\\n\";\ntry {{ include {:?}; }} catch (Throwable $error) {{ echo \"caught\\n\"; }}\necho \"after\\n\";\n",
        fixture.invalid_enum.to_string_lossy()
    );
    let (status, stdout, stderr) = run_stdin(&invalid_driver);
    let invalid_enum = std::fs::canonicalize(&fixture.invalid_enum).unwrap();
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Constant expression contains invalid operations in {} on line 4\n",
            invalid_enum.to_string_lossy()
        )
    );

    let deferred_driver = format!(
        "<?php\ninclude {:?};\necho \"before\\n\";\ntry {{ var_dump(Deferred\\Holder::BAD); }} catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), '@', basename($error->getFile()), ':', $error->getLine(), \"\\n\"; }}\necho \"after\\n\";\n",
        fixture.object_offset.to_string_lossy()
    );
    let (status, stdout, stderr) = run_stdin(&deferred_driver);
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "before\nError:Cannot use [] on objects in constant expression@object-offset.php:11\nafter\n"
    );
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn eval_compile_fatal_and_deferred_error_preserve_source_state_and_origin() {
    let invalid = "<?php\nregister_shutdown_function(function () { echo \"shutdown\\n\"; });\necho \"before\\n\";\ntry { eval('namespace Evaluated; enum Signal: int { case Bad = 1 + $value; }'); } catch (Throwable $error) { echo \"caught\\n\"; }\necho \"after\\n\";\n";
    let (status, stdout, stderr) = run_stdin(invalid);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Constant expression contains invalid operations in Standard input code(4) : eval()'d code on line 1\n"
    );

    let deferred = r#"<?php
eval('namespace Evaluated; enum Signal implements \\ArrayAccess { case Ready; public function offsetGet($key): mixed { echo "offsetGet\\n"; return 42; } public function offsetExists($key): bool { return true; } public function offsetSet($key, $value): void {} public function offsetUnset($key): void {} } class Holder { public const BAD = Signal::Ready[0]; }');
echo "before\n";
try {
    var_dump(Evaluated\Holder::BAD);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '@', $error->getFile(), ':', $error->getLine(), "\n";
}
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(deferred);
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "before\nError:Cannot use [] on objects in constant expression@Standard input code(2) : eval()'d code:1\nafter\n"
    );
    assert!(stderr.is_empty(), "{stderr:?}");
}
