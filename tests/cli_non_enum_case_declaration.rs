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

fn assert_case_fatal(label: &str, source: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{label}: {stderr:?}");
    assert!(stdout.is_empty(), "{label}: {stdout:?}");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Case can only be used in enums in Standard input code on line {line}\n"
        ),
        "{label}"
    );
}

#[test]
fn non_enum_case_uses_classlike_kind_and_case_name_line() {
    let cases = [
        ("class", "<?php\nclass Signal {\n    case Bad;\n}\n", 3),
        (
            "namespace and multiline",
            "<?php\nnamespace Oracle;\nclass Signal {\n    case\n        Bad;\n}\n",
            5,
        ),
        ("trait", "<?php\ntrait Signal {\n    case Bad;\n}\n", 3),
        (
            "interface",
            "<?php\ninterface Signal {\n    case Bad;\n}\n",
            3,
        ),
        (
            "anonymous",
            "<?php\n$object = new class {\n    case Bad;\n};\n",
            3,
        ),
    ];

    for (label, source, line) in cases {
        assert_case_fatal(label, source, line);
    }
}

#[test]
fn non_enum_case_consumes_attributes_values_and_selects_first_case() {
    let cases = [
        (
            "attribute",
            "<?php\nclass Signal {\n    #[Deprecated]\n    case Bad;\n}\n",
            4,
        ),
        (
            "assigned value",
            "<?php\nclass Signal {\n    case Bad = Missing::VALUE;\n}\n",
            3,
        ),
        (
            "first case",
            "<?php\nclass Signal {\n    case First;\n    case Second;\n}\n",
            3,
        ),
    ];

    for (label, source, line) in cases {
        assert_case_fatal(label, source, line);
    }
}

#[test]
fn non_enum_case_preserves_header_and_member_compile_error_order() {
    let cases = [
        (
            "header modifier",
            "<?php\nfinal abstract class Signal {\n    case Bad;\n}\n",
            "Fatal error: Cannot use the final modifier on an abstract class in Standard input code on line 2\n",
        ),
        (
            "method body before",
            "<?php\nclass Signal {\n    public function run(): void {\n        break;\n    }\n    case Bad;\n}\n",
            "Fatal error: 'break' not in the 'loop' or 'switch' context in Standard input code on line 4\n",
        ),
        (
            "method body after",
            "<?php\nclass Signal {\n    case Bad;\n    public function run(): void {\n        break;\n    }\n}\n",
            "Fatal error: Case can only be used in enums in Standard input code on line 3\n",
        ),
        (
            "constant before",
            "<?php\nclass Signal {\n    public const BAD = $value;\n    case Worse;\n}\n",
            "Fatal error: Constant expression contains invalid operations in Standard input code on line 3\n",
        ),
        (
            "constant after",
            "<?php\nclass Signal {\n    case Bad;\n    public const WORSE = $value;\n}\n",
            "Fatal error: Case can only be used in enums in Standard input code on line 3\n",
        ),
        (
            "abstract before",
            "<?php\nclass Signal {\n    private abstract function run();\n    case Bad;\n}\n",
            "Fatal error: Class Signal declares abstract method run() and must therefore be declared abstract in Standard input code on line 3\n",
        ),
        (
            "abstract after",
            "<?php\nclass Signal {\n    case Bad;\n    private abstract function run();\n}\n",
            "Fatal error: Case can only be used in enums in Standard input code on line 3\n",
        ),
        (
            "property type before",
            "<?php\nclass Signal {\n    public callable $value;\n    case Bad;\n}\n",
            "Fatal error: Property Signal::$value cannot have type callable in Standard input code on line 3\n",
        ),
        (
            "property type after",
            "<?php\nclass Signal {\n    case Bad;\n    public callable $value;\n}\n",
            "Fatal error: Case can only be used in enums in Standard input code on line 3\n",
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
fn non_enum_case_keeps_later_syntax_and_invalid_member_grammar() {
    let (status, stdout, stderr) =
        run_stdin("<?php\nclass Signal {\n    case Bad;\n}\nisset(, $value);\n");
    assert_eq!(status, 255);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \",\" in Standard input code on line 5\n"
    );

    let (status, stdout, stderr) = run_stdin("<?php\nclass Signal {\n    public case Bad;\n}\n");
    assert_eq!(status, 255);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \"case\", expecting variable in Standard input code on line 3\n"
    );

    for source in [
        "<?php\nclass Signal {\n    case;\n}\n",
        "<?php\nclass Signal {\n    case Bad\n}\n",
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{stderr:?}");
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("Parse error:"), "{stderr:?}");
        assert!(
            !stderr.contains("Case can only be used in enums"),
            "{stderr:?}"
        );
    }
}

#[test]
fn non_enum_case_is_validated_in_elided_post_return_and_nested_classes() {
    let cases = [
        (
            "elided",
            "<?php\nif (false) {\n    class Signal {\n        case Bad;\n    }\n}\necho \"after\\n\";\n",
            4,
        ),
        (
            "post return",
            "<?php\nfunction loadSignal(): void {\n    return;\n    class Signal {\n        case Bad;\n    }\n}\necho \"after\\n\";\n",
            5,
        ),
        (
            "nested",
            "<?php\nfunction loadSignal(): void {\n    class Signal {\n        case Bad;\n    }\n}\necho \"after\\n\";\n",
            4,
        ),
    ];

    for (label, source, line) in cases {
        assert_case_fatal(label, source, line);
    }
}

struct IncludeFixture {
    directory: std::path::PathBuf,
    invalid_class: std::path::PathBuf,
}

impl IncludeFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_non_enum_case_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let invalid_class = directory.join("invalid-class.php");
        std::fs::write(
            &invalid_class,
            "<?php\nnamespace Included;\nclass Signal {\n    case Bad;\n}\n",
        )
        .unwrap();
        Self {
            directory,
            invalid_class,
        }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn include_case_fatal_preserves_prior_output_shutdown_and_source_origin() {
    let fixture = IncludeFixture::new();
    let source = format!(
        "<?php\nregister_shutdown_function(function () {{ echo \"shutdown\\n\"; }});\necho \"before\\n\";\ntry {{ include {:?}; }} catch (Throwable $error) {{ echo \"caught\\n\"; }}\necho \"after\\n\";\n",
        fixture.invalid_class.to_string_lossy()
    );
    let (status, stdout, stderr) = run_stdin(&source);
    let invalid_class = std::fs::canonicalize(&fixture.invalid_class).unwrap();

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Case can only be used in enums in {} on line 4\n",
            invalid_class.to_string_lossy()
        )
    );
}

#[test]
fn eval_case_fatal_preserves_prior_output_shutdown_and_source_origin() {
    let source = "<?php\nregister_shutdown_function(function () { echo \"shutdown\\n\"; });\necho \"before\\n\";\ntry { eval('namespace Evaluated; class Signal { case Bad; }'); } catch (Throwable $error) { echo \"caught\\n\"; }\necho \"after\\n\";\n";
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Case can only be used in enums in Standard input code(4) : eval()'d code on line 1\n"
    );
}

#[test]
fn valid_enum_switch_and_case_method_controls_remain_executable() {
    let source = r#"<?php
enum State: int {
    case Ready = 7;
}
class Signal {
    public function case(): string { return 'method'; }
    public function value(int $input): string {
        switch ($input) {
            case 1: return 'one';
            default: return 'other';
        }
    }
}
$signal = new Signal();
echo State::Ready->name, '|', State::Ready->value, '|', $signal->case(), '|', $signal->value(1), "\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "Ready|7|method|one\n");
    assert!(stderr.is_empty(), "{stderr:?}");
}
