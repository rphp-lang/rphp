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
fn concrete_class_uses_the_first_ordinary_abstract_method_and_its_line() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
namespace Oracle;
class Broken {
    abstract public function first();
    abstract protected static function second();
}
echo "never\n";
"#,
    );

    assert_eq!(status, 255);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "Fatal error: Class Oracle\\Broken declares abstract method first() and must therefore be declared abstract in Standard input code on line 4\n"
    );
}

#[test]
fn abstract_capable_declaration_kinds_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait ContractTrait { abstract public function fromTrait(); }
interface ContractInterface { public function fromInterface(); }
abstract class ContractBase {
    abstract public function fromBase();
    public abstract string $label { get; }
}
echo "valid\n";
"#,
    );

    assert_eq!(
        (status, stdout, stderr),
        (0, "valid\n".into(), String::new())
    );
}

#[test]
fn constant_branches_and_code_after_return_are_still_validated() {
    for (label, source, expected) in [
        (
            "false-branch",
            "<?php\nif (false) {\n    class ConditionalBroken {\n        abstract public function pending();\n    }\n}\necho \"never\\n\";\n",
            "Fatal error: Class ConditionalBroken declares abstract method pending() and must therefore be declared abstract in Standard input code on line 4\n",
        ),
        (
            "true-else",
            "<?php\nif (true) {\n    echo 'compiled only';\n} else {\n    class ElidedBroken {\n        abstract public function pending();\n    }\n}\n",
            "Fatal error: Class ElidedBroken declares abstract method pending() and must therefore be declared abstract in Standard input code on line 6\n",
        ),
        (
            "after-return",
            "<?php\nfunction load_later() {\n    return;\n    class DeadAfterReturn {\n        abstract public function pending();\n    }\n}\necho \"never\\n\";\n",
            "Fatal error: Class DeadAfterReturn declares abstract method pending() and must therefore be declared abstract in Standard input code on line 5\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(stderr, expected, "{label}");
    }
}

#[test]
fn earlier_link_errors_and_global_parser_compile_errors_keep_priority() {
    let cases = [
        (
            "import",
            "<?php\nnamespace ImportPriority;\nuse DateTime as Broken;\nclass Broken {\n    abstract public function pending();\n}\n",
            "Fatal error: Cannot redeclare class ImportPriority\\Broken (previously declared as local import) in Standard input code on line 4\n",
        ),
        (
            "later-parse",
            "<?php\nclass AbstractFirst { abstract public function pending(); }\nisset(, $value);\n",
            "Parse error: syntax error, unexpected token \",\" in Standard input code on line 3\n",
        ),
        (
            "later-deferred-compile",
            "<?php\nclass AbstractFirst { abstract public function pending(); }\nabstract class ModifierLater {\n    final abstract public function conflict();\n}\n",
            "Fatal error: Cannot use the final modifier on an abstract method in Standard input code on line 4\n",
        ),
    ];

    for (label, source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(stderr, expected, "{label}");
    }
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
            "rphp_abstract_method_staging_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(
            &include,
            "<?php\nnamespace Included;\nclass Broken {\n    abstract public function pending();\n}\n",
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
fn include_fatal_is_uncatchable_and_preserves_prior_and_shutdown_state() {
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
            "Fatal error: Class Included\\Broken declares abstract method pending() and must therefore be declared abstract in {} on line 4\n",
            include.to_string_lossy()
        )
    );
}

#[test]
fn eval_fatal_is_uncatchable_and_preserves_prior_and_shutdown_state() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
register_shutdown_function(function () { echo "shutdown\n"; });
echo "before\n";
try {
    eval("namespace Evaluated;\nclass Broken {\n    abstract public function pending();\n}");
} catch (Throwable $error) {
    echo "caught\n";
}
echo "after\n";
"#,
    );

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Class Evaluated\\Broken declares abstract method pending() and must therefore be declared abstract in Standard input code(5) : eval()'d code on line 3\n"
    );
}
