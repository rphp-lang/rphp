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
fn class_and_method_modifier_errors_use_canonical_spelling_and_terminal_lines() {
    let cases = [
        (
            "class",
            "<?php\nnamespace Oracle;\nfinal\nabstract\nclass Broken {}\n",
            "Fatal error: Cannot use the final modifier on an abstract class in Standard input code on line 4\n",
        ),
        (
            "method",
            "<?php\nnamespace Oracle;\nabstract class Broken {\n    private\n    abstract\n    function HiddenCase();\n}\n",
            "Fatal error: Abstract function Oracle\\Broken::HiddenCase() cannot be declared private in Standard input code on line 6\n",
        ),
        (
            "method-body",
            "<?php\nabstract class Broken {\n    private abstract function hidden() {\n        echo 'never';\n    }\n}\n",
            "Fatal error: Abstract function Broken::hidden() cannot be declared private in Standard input code on line 3\n",
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
fn valid_abstract_and_concrete_modifier_boundaries_remain_accepted() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
abstract class ValidBase {
    public abstract function publicContract();
    protected abstract function protectedContract();
    private function concretePrivate() {}
}
final class ValidLeaf extends ValidBase {
    public function publicContract() {}
    protected function protectedContract() {}
}
trait ValidTrait { private abstract function hidden(); }
interface ValidInterface { public function contract(); }
echo "valid\n";
"#,
    );

    assert_eq!(
        (status, stdout, stderr),
        (0, "valid\n".into(), String::new())
    );
}

#[test]
fn declaration_and_parser_diagnostic_order_is_preserved() {
    let cases = [
        (
            "same-class",
            "<?php\nfinal abstract class Broken {\n    private abstract function hidden();\n}\n",
            "Fatal error: Cannot use the final modifier on an abstract class in Standard input code on line 2\n",
        ),
        (
            "concrete-class",
            "<?php\nclass Broken {\n    private abstract function hidden();\n}\n",
            "Fatal error: Class Broken declares abstract method hidden() and must therefore be declared abstract in Standard input code on line 3\n",
        ),
        (
            "import",
            "<?php\nnamespace ImportPriority;\nuse DateTime as Broken;\nabstract class Broken {\n    private abstract function hidden();\n}\n",
            "Fatal error: Cannot redeclare class ImportPriority\\Broken (previously declared as local import) in Standard input code on line 4\n",
        ),
        (
            "later-parse",
            "<?php\nabstract class Broken { private abstract function hidden(); }\nisset(, $value);\n",
            "Parse error: syntax error, unexpected token \",\" in Standard input code on line 3\n",
        ),
        (
            "later-deferred",
            "<?php\nabstract class First { private abstract function hidden(); }\nfinal abstract class Second {}\n",
            "Fatal error: Cannot use the final modifier on an abstract class in Standard input code on line 3\n",
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
fn constant_branches_and_code_after_return_are_still_validated() {
    let cases = [
        (
            "class-modifier",
            "<?php\nif (false) {\n    final abstract class ConditionalBroken {}\n}\n",
            "Fatal error: Cannot use the final modifier on an abstract class in Standard input code on line 3\n",
        ),
        (
            "private-method",
            "<?php\nfunction load_later() {\n    return;\n    abstract class DeadPrivate {\n        private abstract function hidden();\n    }\n}\n",
            "Fatal error: Abstract function DeadPrivate::hidden() cannot be declared private in Standard input code on line 5\n",
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
    fn new(label: &str, included_source: &str) -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_abstract_modifier_staging_{}_{}_{}",
            std::process::id(),
            identity,
            label
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(&include, included_source).unwrap();
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
fn include_modifier_fatals_are_uncatchable_and_preserve_prior_state() {
    let cases = [
        (
            "class",
            "<?php\nnamespace Included;\nfinal abstract class Broken {}\n",
            "Cannot use the final modifier on an abstract class",
            3,
        ),
        (
            "method",
            "<?php\nnamespace Included;\nabstract class Broken {\n    private abstract function hidden();\n}\n",
            "Abstract function Included\\Broken::hidden() cannot be declared private",
            4,
        ),
    ];

    for (label, source, message, line) in cases {
        let fixture = IncludeFixture::new(label, source);
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

        assert_eq!(output.status.code(), Some(255), "{label}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "before\nshutdown\n",
            "{label}"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!(
                "Fatal error: {message} in {} on line {line}\n",
                include.to_string_lossy()
            ),
            "{label}"
        );
    }
}

#[test]
fn eval_modifier_fatals_are_uncatchable_and_preserve_prior_state() {
    let cases = [
        (
            "class",
            "namespace Evaluated;\nfinal abstract class Broken {}",
            "Cannot use the final modifier on an abstract class",
            2,
        ),
        (
            "method",
            "namespace Evaluated;\nabstract class Broken {\n    private abstract function hidden();\n}",
            "Abstract function Evaluated\\Broken::hidden() cannot be declared private",
            3,
        ),
    ];

    for (label, evaluated, message, line) in cases {
        let source = format!(
            "<?php\nregister_shutdown_function(function () {{ echo \"shutdown\\n\"; }});\necho \"before\\n\";\ntry {{\n    eval({evaluated:?});\n}} catch (Throwable $error) {{\n    echo \"caught\\n\";\n}}\necho \"after\\n\";\n"
        );
        let (status, stdout, stderr) = run_stdin(&source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert_eq!(stdout, "before\nshutdown\n", "{label}");
        assert_eq!(
            stderr,
            format!(
                "Fatal error: {message} in Standard input code(5) : eval()'d code on line {line}\n"
            ),
            "{label}"
        );
    }
}
