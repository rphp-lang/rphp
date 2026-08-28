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
fn direct_enum_abstract_errors_use_the_first_qualified_method_and_line() {
    let cases = [
        (
            "semicolon",
            "<?php\nenum Example {\n    abstract public function foo();\n}\n",
            "Fatal error: Enum method Example::foo() must not be abstract in Standard input code on line 3\n",
        ),
        (
            "namespace",
            "<?php\nnamespace Oracle;\nenum Example {\n    abstract\n    public\n    function MixedCase();\n}\n",
            "Fatal error: Enum method Oracle\\Example::MixedCase() must not be abstract in Standard input code on line 6\n",
        ),
        (
            "body",
            "<?php\nenum Example {\n    abstract protected function body() {\n        echo 'never';\n    }\n}\n",
            "Fatal error: Enum method Example::body() must not be abstract in Standard input code on line 3\n",
        ),
        (
            "first",
            "<?php\nnamespace Oracle;\nenum Example {\n    abstract public function FirstCase();\n    abstract public function second();\n}\n",
            "Fatal error: Enum method Oracle\\Example::FirstCase() must not be abstract in Standard input code on line 4\n",
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
fn earlier_enum_checks_and_later_parser_errors_keep_priority() {
    let cases = [
        (
            "backing",
            "<?php\nenum Example: float {\n    abstract public function foo();\n}\n",
            "Fatal error: Enum backing type must be int or string, float given in Standard input code on line 2\n",
        ),
        (
            "property",
            "<?php\nenum Example {\n    public int $value;\n    abstract public function foo();\n}\n",
            "Fatal error: Enum Example cannot include properties in Standard input code on line 3\n",
        ),
        (
            "case",
            "<?php\nenum Example: int {\n    case Missing;\n    abstract public function foo();\n}\n",
            "Fatal error: Case Missing of backed enum Example must have a value in Standard input code on line 3\n",
        ),
        (
            "import",
            "<?php\nnamespace Oracle;\nuse DateTime as Example;\nenum Example {\n    abstract public function foo();\n}\n",
            "Fatal error: Cannot redeclare class Oracle\\Example (previously declared as local import) in Standard input code on line 4\n",
        ),
        (
            "later-parse",
            "<?php\nenum Example {\n    abstract public function foo();\n}\nisset(, $value);\n",
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
fn parser_deferred_modifier_errors_precede_enum_abstract_validation() {
    let cases = [
        (
            "final",
            "<?php\nenum Example {\n    final abstract public function foo();\n}\n",
            "Fatal error: Cannot use the final modifier on an abstract method in Standard input code on line 3\n",
        ),
        (
            "duplicate",
            "<?php\nenum Example {\n    abstract abstract public function foo();\n}\n",
            "Fatal error: Multiple abstract modifiers are not allowed in Standard input code on line 3\n",
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
fn abstract_validation_precedes_later_enum_method_checks() {
    let cases = [
        (
            "magic",
            "<?php\nenum Example {\n    public function __construct() {}\n    abstract public function foo();\n}\n",
            "Fatal error: Enum method Example::foo() must not be abstract in Standard input code on line 4\n",
        ),
        (
            "reserved",
            "<?php\nenum Example {\n    abstract public function cases();\n}\n",
            "Fatal error: Enum method Example::cases() must not be abstract in Standard input code on line 3\n",
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
fn elided_enum_declarations_are_still_validated() {
    let cases = [
        (
            "constant-false",
            "<?php\nif (false) {\n    enum Example {\n        abstract public function foo();\n    }\n}\n",
            "Fatal error: Enum method Example::foo() must not be abstract in Standard input code on line 4\n",
        ),
        (
            "after-return",
            "<?php\nfunction load_later() {\n    return;\n    enum Example {\n        abstract public function foo();\n    }\n}\n",
            "Fatal error: Enum method Example::foo() must not be abstract in Standard input code on line 5\n",
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
fn concrete_enum_method_boundaries_remain_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
interface Contract { public function run(): string; }
enum Example implements Contract {
    case A;
    public function run(): string { return $this->name . self::helper() . $this->hidden() . $this->finish(); }
    protected static function helper(): int { return 1; }
    private function hidden(): string { return "x"; }
    final public function finish(): string { return "f"; }
}
echo Example::A->run(), "\n";
"#,
    );

    assert_eq!(
        (status, stdout, stderr),
        (0, "A1xf\n".into(), String::new())
    );
}

struct IncludeFixture {
    directory: std::path::PathBuf,
    include: std::path::PathBuf,
    driver: std::path::PathBuf,
}

impl IncludeFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_abstract_enum_method_staging_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(
            &include,
            "<?php\nnamespace Included;\nenum Example {\n    abstract public function foo();\n}\n",
        )
        .unwrap();
        let driver = directory.join("driver.php");
        let source = format!(
            r#"<?php
register_shutdown_function(function () {{ echo "shutdown\n"; }});
echo "before\n";
try {{
    include {include:?};
}} catch (Throwable $error) {{
    echo "caught\n";
}}
echo "after\n";
"#,
            include = include.to_string_lossy(),
        );
        std::fs::write(&driver, source).unwrap();
        Self {
            directory,
            include,
            driver,
        }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn include_enum_abstract_fatal_is_uncatchable_and_preserves_prior_state() {
    let fixture = IncludeFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
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
        ])
        .arg(&fixture.driver)
        .output()
        .expect("rphp should run include fixture");
    let include = std::fs::canonicalize(&fixture.include).unwrap();

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "before\nshutdown\n"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "Fatal error: Enum method Included\\Example::foo() must not be abstract in {} on line 4\n",
            include.to_string_lossy()
        )
    );
}

#[test]
fn eval_enum_abstract_fatal_is_uncatchable_and_preserves_prior_state() {
    let source = "<?php\nregister_shutdown_function(function () { echo \"shutdown\\n\"; });\necho \"before\\n\";\ntry {\n    eval(\"namespace Evaluated;\\nenum Example {\\n    abstract public function foo();\\n}\");\n} catch (Throwable $error) {\n    echo \"caught\\n\";\n}\necho \"after\\n\";\n";
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Enum method Evaluated\\Example::foo() must not be abstract in Standard input code(5) : eval()'d code on line 3\n"
    );
}
